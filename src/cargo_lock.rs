//! Typed model of a component's `Cargo.lock`.
//!
//! Deserialized with serde through the TOML deserializer. The lock is
//! machine-generated and comment-free apart from Cargo's fixed header;
//! reserialization reproduces Cargo's own canonical lock rendering
//! (header line included) through the lock style of
//! [`crate::toml_pretty::PrettyPrinter`].

use serde::{Deserialize, Serialize};
use toml::Table;

use crate::cargo_manifest::{DependencyName, GitReference, PackageVersion};
use crate::error::Error;
use crate::toml_pretty::PrettyPrinter;
use crate::types::{CommitIdentifier, RepositoryUrl, TomlText};

/// A `Cargo.lock` document (lock format versions 3 and 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoLock {
    version: u32,
    #[serde(rename = "package")]
    packages: Vec<LockedPackage>,
    #[serde(flatten)]
    remainder: Table,
}

impl CargoLock {
    /// Deserialize a lock from TOML text.
    pub fn from_toml_text(text: &str) -> Result<Self, Error> {
        todo!("toml deserializer via serde")
    }

    /// Every git-sourced package entry, with its parsed pin.
    pub fn git_packages(&self) -> Vec<(DependencyName, GitPin)> {
        todo!()
    }

    /// Repin the named git package in-type, returning the previous locked
    /// revision.
    ///
    /// Sets the locked revision, rewrites the source reference query
    /// (`?branch=...`) to `reference`, and synchronizes the recorded package
    /// version to `version_at_target` — the version the dependency's own
    /// manifest declares at the target revision.
    ///
    /// This covers rev-only drift. If the dependency's *own dependency set*
    /// changed at the target revision, the typed edit cannot invent the new
    /// transitive entries; the build-verify stage surfaces that as a
    /// collected failure (ARCHITECTURE.md §4, §14).
    pub fn repin_git_package(
        &mut self,
        name: &DependencyName,
        reference: GitReference,
        revision: CommitIdentifier,
        version_at_target: PackageVersion,
    ) -> Result<CommitIdentifier, Error> {
        todo!()
    }

    /// Reserialize through the pretty printer in Cargo's canonical lock
    /// rendering.
    pub fn to_pretty_toml(&self, printer: &PrettyPrinter) -> Result<TomlText, Error> {
        todo!()
    }
}

/// One `[[package]]` entry. External field names preserved at the serde
/// boundary; entries without a `source` are path/workspace members.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedPackage {
    name: DependencyName,
    version: PackageVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<SourceText>,
    #[serde(flatten)]
    remainder: Table,
}

/// The raw `source` field text, e.g.
/// `git+https://github.com/LiGoldragon/signal-frame.git?branch=main#<rev>`.
/// Parsed into [`GitPin`] by a typed parser, never split ad hoc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceText(String);

impl SourceText {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse a git source into its typed pin, or `None` for registry
    /// sources.
    pub fn git_pin(&self) -> Result<Option<GitPin>, Error> {
        todo!("winnow parser over the git+<url>?<query>#<rev> shape")
    }
}

/// A parsed git lock pin: where the package comes from and the exact commit
/// it is locked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPin {
    url: RepositoryUrl,
    reference: GitReference,
    revision: CommitIdentifier,
}

impl GitPin {
    pub fn url(&self) -> &RepositoryUrl {
        &self.url
    }

    pub fn reference(&self) -> &GitReference {
        &self.reference
    }

    pub fn revision(&self) -> &CommitIdentifier {
        &self.revision
    }
}
