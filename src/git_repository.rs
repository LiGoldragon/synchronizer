//! Git boundary for one component.
//!
//! A [`GitRepository`] is a component's configured local clone used as a
//! git *object store* and push origin — never as a working copy. The tool
//! reads files at fetched revisions from the object database, builds bump
//! commits as tree objects on top of the remote `main` tip, and pushes only
//! the tool-owned `synchronizer` branch.
//!
//! Invariant (test seed): no operation on this type touches the checkout's
//! working copy, index, current branch, or any agent-owned branch, and no
//! operation writes `main` — locally or remotely.

use std::path::PathBuf;

use crate::error::Error;
use crate::types::{CommitIdentifier, ComponentName, RepositoryUrl};

/// One component's git surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepository {
    component: ComponentName,
    clone_path: PathBuf,
    remote_url: RepositoryUrl,
}

impl GitRepository {
    /// Open the configured clone at `clone_path`.
    pub fn open(
        component: ComponentName,
        clone_path: PathBuf,
        remote_url: RepositoryUrl,
    ) -> Result<Self, Error> {
        todo!()
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    /// The latest pushed `main` tip, queried read-only from the remote
    /// (ls-remote; nothing is written anywhere).
    pub fn remote_main_tip(&self) -> Result<CommitIdentifier, Error> {
        todo!()
    }

    /// Fetch `revision` into the local object store so files at that
    /// revision can be read.
    pub fn fetch(&self, revision: &CommitIdentifier) -> Result<(), Error> {
        todo!()
    }

    /// Read one file's text at `revision` from the object store. `None`
    /// when the file does not exist at that revision.
    pub fn file_at(
        &self,
        revision: &CommitIdentifier,
        path: &RepositoryFilePath,
    ) -> Result<Option<String>, Error> {
        todo!()
    }

    /// Build one commit on top of `base` with the given file contents
    /// replaced, entirely at the object level — no working copy is
    /// created or modified.
    pub fn commit_file_edits(
        &self,
        base: &CommitIdentifier,
        edits: &[FileEdit],
        message: &CommitMessage,
    ) -> Result<CommitIdentifier, Error> {
        todo!()
    }

    /// Point the tool-owned `synchronizer` branch at `commit` and push it
    /// to the remote, overwriting any previous run's branch. The branch is
    /// staging surface owned by this tool; force-updating it is the
    /// designed behavior.
    pub fn push_synchronizer_branch(&self, commit: &CommitIdentifier) -> Result<(), Error> {
        todo!()
    }
}

/// A path inside a repository tree (relative to the repository root),
/// e.g. `Cargo.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryFilePath(String);

impl RepositoryFilePath {
    pub fn cargo_manifest() -> Self {
        Self("Cargo.toml".to_string())
    }

    pub fn cargo_lock() -> Self {
        Self("Cargo.lock".to_string())
    }

    pub fn flake_manifest() -> Self {
        Self("flake.nix".to_string())
    }

    pub fn flake_lock() -> Self {
        Self("flake.lock".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One file replacement inside a bump commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    path: RepositoryFilePath,
    content: String,
}

impl FileEdit {
    pub fn new(path: RepositoryFilePath, content: String) -> Self {
        Self { path, content }
    }
}

/// A synchronizer commit message, rendered from the applied bumps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMessage(String);

impl CommitMessage {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
