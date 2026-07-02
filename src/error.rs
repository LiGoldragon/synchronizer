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
