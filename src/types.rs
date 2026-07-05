//! Domain newtypes shared across the crate.
//!
//! Every value with a role in the model gets its own type; a bare `String`
//! never crosses a module boundary.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use nota::{NotaDecode, NotaEncode};
use winnow::Parser;
use winnow::combinator::opt;
use winnow::token::take_while;

/// Name of a participating component repository. Matches the GitHub
/// repository name exactly, e.g. `signal-router`.
///
/// This is a repository identity, not a crate name: a repository may publish
/// a crate under a different package name (`codec-repository` publishes `nota`).
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

    /// Whether this text has the shape of a full git object id. Used by the
    /// flake.nix URL scanner to distinguish a revision pin segment from a
    /// branch or tag reference.
    pub fn is_full_object_id(text: &str) -> bool {
        text.len() == 40 && text.chars().all(|character| character.is_ascii_hexdigit())
    }
}

/// A git branch name.
///
/// The tool carries no branch-name constant of its own: both the mainline
/// branch (the default version target) and the staging branch (the tool-owned
/// branch every bump is pushed to) are supplied by configuration through
/// [`crate::configuration::BranchScheme`]. Nothing here assumes `main` or
/// `synchronizer`.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
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

    /// The owner/repository identity of the URL, used to match a git
    /// dependency or flake input against the configured component set.
    /// Package and input names never participate in matching.
    pub fn repository_identity(&self) -> Result<RepositoryIdentity, crate::Error> {
        let mut input = self.0.as_str();
        RepositoryIdentity::parse(&mut input).map_err(|_| crate::Error::RepositoryUrlUnparseable {
            url: self.0.clone(),
        })
    }

    /// The repository name segment of the URL.
    pub fn repository_name(&self) -> Result<ComponentName, crate::Error> {
        Ok(self.repository_identity()?.repository)
    }
}

/// The owner and repository name a remote URL points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIdentity {
    pub owner: String,
    pub repository: ComponentName,
}

impl RepositoryIdentity {
    /// Winnow grammar over the repository-URL shapes this workspace uses:
    /// `https://<host>/<owner>/<repository>(.git)?` and
    /// `ssh://git@<host>/<owner>/<repository>(.git)?`. The identity is the
    /// final two path segments.
    fn parse(input: &mut &str) -> winnow::Result<Self> {
        let scheme = take_while(1.., |character: char| {
            character.is_ascii_alphanumeric() || character == '+'
        });
        let separator = "://";
        let segment = || take_while(1.., |character: char| character != '/' && character != '?');
        let (_, _, _, _, owner, _, repository) = (
            scheme,
            separator,
            segment(), // host, possibly with a user@ prefix
            '/',
            segment(),
            '/',
            take_while(1.., |character: char| character != '/' && character != '?'),
        )
            .parse_next(input)?;
        let _ = opt('/').parse_next(input)?;
        let repository = repository.strip_suffix(".git").unwrap_or(repository);
        Ok(Self {
            owner: owner.to_string(),
            repository: ComponentName::new(repository),
        })
    }
}

/// A CriomOS role name naming the build-verify host indirectly,
/// e.g. `NixBuilder`. The tool never holds a hostname of its own; the role
/// atom is matched against the service kinds the cluster proposal authors
/// per node.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct BuilderRole(String);

impl BuilderRole {
    pub fn new(role: impl Into<String>) -> Self {
        Self(role.into())
    }

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
    pub fn new(host: impl Into<String>) -> Self {
        Self(host.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A flake reference such as `github:LiGoldragon/signal-frame/<revision>`.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct FlakeReference(String);

impl FlakeReference {
    pub fn new(reference: impl Into<String>) -> Self {
        Self(reference.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The human name recorded as author and committer on every bump commit.
/// Supplied by configuration ([`crate::configuration::CommitAuthor`]); the
/// tool holds no author identity of its own.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct AuthorName(String);

impl AuthorName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The email recorded as author and committer on every bump commit. Supplied
/// by configuration; the tool holds no email of its own (no `criome.net` or
/// any other domain baked in).
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct AuthorEmail(String);

impl AuthorEmail {
    pub fn new(email: impl Into<String>) -> Self {
        Self(email.into())
    }

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

/// Rendered TOML text produced by a format-preserving document edit.
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
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub fn as_path_buffer(&self) -> PathBuf {
        PathBuf::from(&self.0)
    }
}

/// A moment in unix seconds, as carried by the run report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn now() -> Self {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before the unix epoch");
        Self(elapsed.as_secs())
    }

    pub fn from_unix_seconds(seconds: u64) -> Self {
        Self(seconds)
    }
}
