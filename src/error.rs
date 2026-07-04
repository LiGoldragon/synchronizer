//! Crate boundary error.
//!
//! [`Error`] carries only run-fatal and infrastructural failures. A
//! per-repository failure during the ascent (a failed bump, push, or verify)
//! is *not* an `Error`: it is collected as report data
//! ([`crate::report::Failure`]) and the run keeps going.

use std::path::PathBuf;

use thiserror::Error;

use crate::topology::PinLayer;
use crate::types::{BuilderRole, ComponentName, FlakeReference};

/// The synchronizer crate's typed boundary error.
#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration file unreadable: {path}: {source}")]
    ConfigurationUnreadable {
        path: PathBuf,
        source: std::io::Error,
    },

    /// The configuration document failed NOTA decoding through the canonical
    /// codec.
    #[error("configuration decode: {detail}")]
    ConfigurationDecode { detail: String },

    #[error("unknown component: {0:?}")]
    UnknownComponent(ComponentName),

    /// A remote URL did not parse as `scheme://host/owner/repository`.
    #[error("repository url unparseable: {url}")]
    RepositoryUrlUnparseable { url: String },

    /// A pin surface failed typed deserialization (serde for TOML and JSON,
    /// the URL parser for flake.nix input URLs).
    #[error("manifest decode: {component:?} {layer:?}: {detail}")]
    ManifestDecode {
        component: ComponentName,
        layer: PinLayer,
        detail: String,
    },

    /// A pin surface failed reserialization.
    #[error("manifest encode: {component:?} {layer:?}: {detail}")]
    ManifestEncode {
        component: ComponentName,
        layer: PinLayer,
        detail: String,
    },

    /// The dependency edge referenced by an operation is not a git
    /// dependency on a configured component.
    #[error("not a component dependency: {consumer:?} -> {dependency}")]
    NotComponentDependency {
        consumer: ComponentName,
        dependency: String,
    },

    /// A pin the mechanical bump must not touch: deliberately rev- or
    /// tag-pinned, or a lock package recorded by several same-name git
    /// entries at genuinely different revisions (no single target rev
    /// repins them coherently). Bumping would emit an invalid or lying
    /// manifest, so the bump fails loud and is collected; the pin is left
    /// alone. (Several same-name *manifest* entries that all follow one
    /// producer are not unbumpable — they are redirected coherently.)
    #[error("unbumpable pin: {consumer:?} -> {dependency}: {reason}")]
    UnbumpablePin {
        consumer: ComponentName,
        dependency: String,
        reason: UnbumpablePinReason,
    },

    /// A consumer pins a producer whose tip is unavailable this run — its
    /// fetch or load failed — so no target revision exists for the edge.
    /// Collected as a Resolve failure; the ascent continues.
    #[error("producer unavailable this run (its fetch or load failed): {producer:?}")]
    ProducerUnavailable { producer: ComponentName },

    /// The discovered dependency graph is not a DAG. Run-fatal: a cycle
    /// admits no topological ascent.
    #[error("dependency cycle among: {members:?}")]
    DependencyCycle { members: Vec<ComponentName> },

    /// A git operation against a component's clone or remote failed.
    #[error("git {operation:?} on {component:?}: {detail}")]
    Git {
        component: ComponentName,
        operation: GitOperation,
        detail: String,
    },

    /// `nix flake prefetch` failed — the flake layer's single external
    /// command boundary.
    #[error("narHash prefetch of {reference:?}: {detail}")]
    NarHashPrefetch {
        reference: FlakeReference,
        detail: String,
    },

    /// The cluster role directory could not resolve the builder role to a
    /// host.
    #[error("builder role {role:?} unresolved: {detail}")]
    RoleUnresolved { role: BuilderRole, detail: String },

    /// The controlled transitive-lock fallback (`cargo update -p <package>
    /// --precise <revision>`) failed to produce a refreshed lock.
    #[error("transitive lock resolution for {component:?}: {detail}")]
    TransitiveLockResolution {
        component: ComponentName,
        detail: String,
    },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// The git operation that failed, for [`Error::Git`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOperation {
    RemoteQuery,
    Fetch,
    ObjectRead,
    Commit,
    Push,
}

/// Why a pin cannot be bumped mechanically, for [`Error::UnbumpablePin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnbumpablePinReason {
    /// The entry pins an exact revision on purpose (`rev = "..."` /
    /// `?rev=`); a mechanical bump would override a deliberate pin or
    /// emit an invalid `branch` + `rev` combination.
    DeliberateRevisionPin,
    /// The entry pins a tag on purpose; a mechanical bump would lie about
    /// what the tag names.
    DeliberateTagPin,
    /// A lock records the producer under several same-name git entries at
    /// genuinely different revisions; no single target rev repins them
    /// coherently, and addressing by name would silently alias the first.
    MultipleEntries,
}

impl std::fmt::Display for UnbumpablePinReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::DeliberateRevisionPin => "the entry deliberately pins an exact revision",
            Self::DeliberateTagPin => "the entry deliberately pins a tag",
            Self::MultipleEntries => "several same-name entries pin the producer",
        };
        formatter.write_str(text)
    }
}
