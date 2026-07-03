//! Git boundary for one component.
//!
//! A [`GitRepository`] is a component's configured local clone used as a
//! git *object store* and push origin — never as a working copy. The tool
//! reads files at fetched revisions from the object database, builds bump
//! commits as blob/tree/commit objects on top of the remote `main` tip
//! (gix, in-process), and pushes only the tool-owned `synchronizer` branch.
//!
//! Object operations are in-process gix (psyche default: typed git
//! library, commits at the object level, working copies untouched).
//! Transport — ls-remote, fetch, push — is a typed invocation of git
//! plumbing, because gix 0.85 implements no push and its network transports
//! are not needed for anything else; the invocation shape mirrors the
//! `NixFlakePrefetch` boundary. Fetched tips land in the neutral
//! `refs/synchronizer/*` namespace so no branch, remote-tracking ref, index,
//! or working copy is touched.
//!
//! Invariant (test seed): no operation on this type touches the checkout's
//! working copy, index, current branch, or any agent-owned branch, and no
//! operation writes `main` — locally or remotely.

use std::path::PathBuf;

use gix::objs::tree::EntryKind;

use crate::configuration::{BranchScheme, CommitAuthor};
use crate::error::{Error, GitOperation};
use crate::types::{CommitIdentifier, ComponentName, RepositoryUrl};

/// The git surface the driver needs from one component. [`GitRepository`]
/// is the production implementation; fixture implementations drive the
/// ascent in tests without a network or a real object store.
pub trait ComponentRepository {
    /// The latest pushed `main` tip, queried read-only from the remote.
    fn remote_main_tip(&self) -> Result<CommitIdentifier, Error>;

    /// The latest pushed staging-branch tip, queried read-only from the
    /// remote, or `None` when the staging branch does not exist. A coordinated
    /// cross-branch run reads an already-staged component at this tip and
    /// resolves its consumers to it (the staging branch), rather than the
    /// mainline. Absence means the component was not pre-staged this run.
    fn remote_staging_tip(&self) -> Result<Option<CommitIdentifier>, Error>;

    /// Fetch `revision` into the local object store so files at that
    /// revision can be read.
    fn fetch(&self, revision: &CommitIdentifier) -> Result<(), Error>;

    /// Read one file's text at `revision` from the object store. `None`
    /// when the file does not exist at that revision.
    fn file_at(
        &self,
        revision: &CommitIdentifier,
        path: &RepositoryFilePath,
    ) -> Result<Option<String>, Error>;

    /// Every file of the tree at `revision`, for materializing a scratch
    /// tree (the transitive-lock fallback needs a full source tree for
    /// `cargo update`).
    fn tree_files_at(&self, revision: &CommitIdentifier) -> Result<Vec<TreeFile>, Error>;

    /// Build one commit on top of `base` with the given file contents
    /// replaced, entirely at the object level — no working copy is created
    /// or modified.
    fn commit_file_edits(
        &self,
        base: &CommitIdentifier,
        edits: &[FileEdit],
        message: &CommitMessage,
    ) -> Result<CommitIdentifier, Error>;

    /// Point the tool-owned `synchronizer` branch at `commit` on the remote,
    /// overwriting any previous run's branch. The branch is staging surface
    /// owned by this tool; force-updating it is the designed behavior.
    fn push_synchronizer_branch(&self, commit: &CommitIdentifier) -> Result<(), Error>;
}

/// One component's production git surface.
pub struct GitRepository {
    component: ComponentName,
    clone_path: PathBuf,
    remote_url: RepositoryUrl,
    branch_scheme: BranchScheme,
    commit_author: CommitAuthor,
    repository: gix::Repository,
}

impl GitRepository {
    /// Open the configured clone at `clone_path`, bound to the configured
    /// branch scheme (which mainline branch to query, which staging branch to
    /// push) and commit author.
    pub fn open(
        component: ComponentName,
        clone_path: PathBuf,
        remote_url: RepositoryUrl,
        branch_scheme: BranchScheme,
        commit_author: CommitAuthor,
    ) -> Result<Self, Error> {
        let repository = gix::open(&clone_path).map_err(|error| Error::Git {
            component: component.clone(),
            operation: GitOperation::ObjectRead,
            detail: format!("open {}: {error}", clone_path.display()),
        })?;
        Ok(Self {
            component,
            clone_path,
            remote_url,
            branch_scheme,
            commit_author,
            repository,
        })
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    fn git_error(&self, operation: GitOperation, detail: impl Into<String>) -> Error {
        Error::Git {
            component: self.component.clone(),
            operation,
            detail: detail.into(),
        }
    }

    /// Run one git plumbing command against the clone and return stdout.
    fn run_plumbing(&self, operation: GitOperation, arguments: &[&str]) -> Result<String, Error> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.clone_path)
            .args(arguments)
            .output()
            .map_err(|error| self.git_error(operation, error.to_string()))?;
        if !output.status.success() {
            return Err(self.git_error(
                operation,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn commit_of(&self, revision: &CommitIdentifier) -> Result<gix::Commit<'_>, Error> {
        let object_id = gix::ObjectId::from_hex(revision.as_str().as_bytes())
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?;
        self.repository
            .find_object(object_id)
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?
            .try_into_commit()
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))
    }
}

impl ComponentRepository for GitRepository {
    fn remote_main_tip(&self) -> Result<CommitIdentifier, Error> {
        let mainline_ref = format!("refs/heads/{}", self.branch_scheme.mainline().as_str());
        let stdout = self.run_plumbing(
            GitOperation::RemoteQuery,
            &["ls-remote", self.remote_url.as_str(), mainline_ref.as_str()],
        )?;
        let tip = stdout
            .split_whitespace()
            .next()
            .filter(|tip| CommitIdentifier::is_full_object_id(tip))
            .ok_or_else(|| {
                self.git_error(
                    GitOperation::RemoteQuery,
                    format!("no {} on {}", mainline_ref, self.remote_url.as_str()),
                )
            })?;
        Ok(CommitIdentifier::new(tip))
    }

    fn remote_staging_tip(&self) -> Result<Option<CommitIdentifier>, Error> {
        let staging_ref = format!("refs/heads/{}", self.branch_scheme.staging().as_str());
        let stdout = self.run_plumbing(
            GitOperation::RemoteQuery,
            &["ls-remote", self.remote_url.as_str(), staging_ref.as_str()],
        )?;
        // An absent staging branch answers with empty output — data, not a
        // failure: the component simply was not pre-staged this run.
        Ok(stdout
            .split_whitespace()
            .next()
            .filter(|tip| CommitIdentifier::is_full_object_id(tip))
            .map(CommitIdentifier::new))
    }

    fn fetch(&self, revision: &CommitIdentifier) -> Result<(), Error> {
        // The fetched tip lands in the neutral refs/synchronizer namespace:
        // not a branch, not a remote-tracking ref, invisible to checkout
        // tooling.
        let refspec = format!("+{}:refs/synchronizer/fetched", revision.as_str());
        self.run_plumbing(
            GitOperation::Fetch,
            &[
                "fetch",
                "--no-tags",
                self.remote_url.as_str(),
                refspec.as_str(),
            ],
        )?;
        Ok(())
    }

    fn file_at(
        &self,
        revision: &CommitIdentifier,
        path: &RepositoryFilePath,
    ) -> Result<Option<String>, Error> {
        let commit = self.commit_of(revision)?;
        let tree = commit
            .tree()
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?;
        let Some(entry) = tree
            .lookup_entry_by_path(path.as_str())
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?
        else {
            return Ok(None);
        };
        let blob = entry
            .object()
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?;
        let text = String::from_utf8(blob.data.to_vec())
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?;
        Ok(Some(text))
    }

    fn tree_files_at(&self, revision: &CommitIdentifier) -> Result<Vec<TreeFile>, Error> {
        let commit = self.commit_of(revision)?;
        let tree = commit
            .tree()
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?;
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse()
            .breadthfirst(&mut recorder)
            .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?;
        let mut files = Vec::new();
        for entry in recorder.records {
            if !entry.mode.is_blob() {
                continue;
            }
            let blob = self
                .repository
                .find_object(entry.oid)
                .map_err(|error| self.git_error(GitOperation::ObjectRead, error.to_string()))?;
            files.push(TreeFile {
                path: RepositoryFilePath::new(entry.filepath.to_string()),
                content: blob.data.to_vec(),
            });
        }
        Ok(files)
    }

    fn commit_file_edits(
        &self,
        base: &CommitIdentifier,
        edits: &[FileEdit],
        message: &CommitMessage,
    ) -> Result<CommitIdentifier, Error> {
        let commit_error = |detail: String| Error::Git {
            component: self.component.clone(),
            operation: GitOperation::Commit,
            detail,
        };
        let base_commit = self.commit_of(base)?;
        let base_tree = base_commit
            .tree()
            .map_err(|error| commit_error(error.to_string()))?;
        let mut editor = self
            .repository
            .edit_tree(base_tree.id())
            .map_err(|error| commit_error(error.to_string()))?;
        for edit in edits {
            let blob = self
                .repository
                .write_blob(edit.content.as_bytes())
                .map_err(|error| commit_error(error.to_string()))?;
            editor
                .upsert(edit.path.as_str(), EntryKind::Blob, blob.detach())
                .map_err(|error| commit_error(error.to_string()))?;
        }
        let tree = editor
            .write()
            .map_err(|error| commit_error(error.to_string()))?;
        let signature = gix::actor::Signature {
            name: self.commit_author.name().as_str().into(),
            email: self.commit_author.email().as_str().into(),
            time: gix::date::Time::now_local_or_utc(),
        };
        let commit = gix::objs::Commit {
            tree: tree.detach(),
            parents: [base_commit.id().detach()].into(),
            author: signature.clone(),
            committer: signature,
            encoding: None,
            message: message.as_str().into(),
            extra_headers: Vec::new(),
        };
        let commit_id = self
            .repository
            .write_object(&commit)
            .map_err(|error| commit_error(error.to_string()))?;
        Ok(CommitIdentifier::new(commit_id.detach().to_string()))
    }

    fn push_synchronizer_branch(&self, commit: &CommitIdentifier) -> Result<(), Error> {
        let staging = self.branch_scheme.staging();
        let refspec = format!("+{}:refs/heads/{}", commit.as_str(), staging.as_str());
        self.run_plumbing(
            GitOperation::Push,
            &["push", self.remote_url.as_str(), refspec.as_str()],
        )?;
        Ok(())
    }
}

/// A path inside a repository tree (relative to the repository root),
/// e.g. `Cargo.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryFilePath(String);

impl RepositoryFilePath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

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

/// One file of a repository tree, as read from the object store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFile {
    pub path: RepositoryFilePath,
    pub content: Vec<u8>,
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

    pub fn path(&self) -> &RepositoryFilePath {
        &self.path
    }

    pub fn content(&self) -> &str {
        &self.content
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
