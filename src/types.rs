//! Domain newtypes shared across the crate.
//!
//! Every value with a role in the model gets its own type; a bare `String`
//! never crosses a module boundary.

use std::path::PathBuf;

use nota::{NotaDecode, NotaEncode};

/// Name of a participating component repository. Matches the GitHub
/// repository name exactly, e.g. `signal-router`.
///
/// This is a repository identity, not a crate name: a repository may publish
/// a crate under a different package name (`nota-next` publishes `nota`).
/// Topology matching always goes through repository identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, NotaDecode, NotaEncode)]
pub struct ComponentName(String);

impl ComponentName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A git commit identifier (full 40-hex object id).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NotaDecode, NotaEncode)]
pub struct CommitIdentifier(String);

impl CommitIdentifier {
    pub fn new(identifier: impl Into<String>) -> Self {
        Self(identifier.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A git branch name.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct BranchName(String);

impl BranchName {
    /// The tool-owned staging branch every bump is pushed to.
    ///
    /// This is the only branch the tool ever writes. It is a constant of the
    /// design, not a configuration parameter.
    pub fn synchronizer() -> Self {
        Self("synchronizer".to_string())
    }

    /// The branch whose remote tip is the default version target.
    pub fn main() -> Self {
        Self("main".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A remote repository URL, e.g. `https://github.com/LiGoldragon/signal-router.git`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryUrl(String);

impl RepositoryUrl {
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The repository name segment of the URL, used to match a git
    /// dependency against the configured component set.
    pub fn repository_name(&self) -> Result<ComponentName, crate::Error> {
        todo!("parse the trailing owner/repo segment; strip a `.git` suffix")
    }
}

/// A CriomOS role name naming the build-verify host indirectly,
/// e.g. `Builder`. The tool never holds a hostname of its own.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct BuilderRole(String);

impl BuilderRole {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A host resolved from a [`BuilderRole`] by a cluster role directory.
/// Only ever produced by role resolution, never authored in configuration
/// or source.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct BuilderHost(String);

impl BuilderHost {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A flake reference such as `github:LiGoldragon/CriomOS-test-cluster`.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct FlakeReference(String);

impl FlakeReference {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An SRI narHash as produced by `nix flake prefetch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarHash(String);

impl NarHash {
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Rendered TOML text produced by the pretty printer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlText(String);

impl TomlText {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An absolute filesystem path decoded from configuration.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct AbsolutePath(String);

impl AbsolutePath {
    pub fn as_path_buffer(&self) -> PathBuf {
        PathBuf::from(&self.0)
    }
}

/// A moment in unix seconds, as carried by the run report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn now() -> Self {
        todo!("system clock, unix seconds")
    }
}
