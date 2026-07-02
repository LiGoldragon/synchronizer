//! Controlled transitive-lock fallback.
//!
//! A typed `Cargo.lock` repin covers rev/version/branch of existing
//! entries; it cannot invent entries when the bumped dependency's *own
//! dependency set* changed at the target revision. The psyche-accepted
//! fallback for exactly that gap is `cargo update -p <package> --precise
//! <revision>`: the tool materializes the consumer's tree at the bump base
//! into a scratch directory (from the object store — no working copy is
//! touched), applies the typed manifest and lock edits, lets cargo complete
//! the transitive graph, and reads the refreshed lock back as the commit
//! content.

use std::path::Path;

use crate::cargo_manifest::DependencyName;
use crate::error::Error;
use crate::git_repository::TreeFile;
use crate::types::{CommitIdentifier, ComponentName, TomlText};

/// One fallback request: everything the resolver needs to rebuild a
/// complete lock for the consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitiveLockRequest {
    /// The consumer whose lock has the gap.
    pub consumer: ComponentName,
    /// The consumer's full source tree at the bump base revision.
    pub base_tree: Vec<TreeFile>,
    /// The consumer's `Cargo.toml` with the typed edits applied.
    pub edited_manifest: TomlText,
    /// The consumer's `Cargo.lock` with the typed edits applied so far.
    pub edited_lock: TomlText,
    /// The bumped package the gap belongs to.
    pub package: DependencyName,
    /// The exact revision the package must lock to.
    pub revision: CommitIdentifier,
}

/// The transitive-lock boundary. Production shells `cargo update`;
/// fixtures stand in during ascent tests.
pub trait TransitiveLockResolver {
    /// Produce a complete `Cargo.lock` for the request.
    fn resolve_lock(&self, request: &TransitiveLockRequest) -> Result<TomlText, Error>;
}

/// The production resolver: `cargo update -p <package> --precise <rev>` in
/// a scratch materialization of the consumer tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoUpdatePrecise {
    /// The cargo binary to invoke, normally `cargo` from PATH.
    cargo_binary: String,
}

impl CargoUpdatePrecise {
    pub fn from_path_environment() -> Self {
        Self {
            cargo_binary: "cargo".to_string(),
        }
    }

    fn materialize(&self, directory: &Path, request: &TransitiveLockRequest) -> Result<(), Error> {
        for file in &request.base_tree {
            let path = directory.join(file.path.as_str());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &file.content)?;
        }
        std::fs::write(
            directory.join("Cargo.toml"),
            request.edited_manifest.as_str(),
        )?;
        std::fs::write(directory.join("Cargo.lock"), request.edited_lock.as_str())?;
        Ok(())
    }
}

impl TransitiveLockResolver for CargoUpdatePrecise {
    fn resolve_lock(&self, request: &TransitiveLockRequest) -> Result<TomlText, Error> {
        let failure = |detail: String| Error::TransitiveLockResolution {
            component: request.consumer.clone(),
            detail,
        };
        let scratch = std::env::temp_dir().join(format!(
            "synchronizer-lock-{}-{}",
            request.consumer.as_str(),
            std::process::id()
        ));
        if scratch.exists() {
            std::fs::remove_dir_all(&scratch)?;
        }
        std::fs::create_dir_all(&scratch)?;
        let outcome = self.materialize(&scratch, request).and_then(|()| {
            let output = std::process::Command::new(&self.cargo_binary)
                .current_dir(&scratch)
                .args([
                    "update",
                    "-p",
                    request.package.as_str(),
                    "--precise",
                    request.revision.as_str(),
                ])
                .output()
                .map_err(|error| failure(error.to_string()))?;
            if !output.status.success() {
                return Err(failure(String::from_utf8_lossy(&output.stderr).to_string()));
            }
            let refreshed = std::fs::read_to_string(scratch.join("Cargo.lock"))
                .map_err(|error| failure(format!("refreshed lock unreadable: {error}")))?;
            Ok(TomlText::new(refreshed))
        });
        let _ = std::fs::remove_dir_all(&scratch);
        outcome
    }
}
