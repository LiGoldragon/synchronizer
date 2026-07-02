//! Typed model of a component's `flake.lock`.
//!
//! The lock is JSON, deserialized with serde into typed data. Repinning an
//! input happens in-type: the new revision is set on the locked node, the
//! narHash is obtained through the [`NarHashSource`] boundary — the *only*
//! external command in the pin-editing path — and the document is
//! reserialized in Nix's own lock rendering. External field names
//! (`narHash`, `lastModified`) are preserved at the serde boundary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::types::{BranchName, CommitIdentifier, ComponentName, NarHash};

/// A `flake.lock` document (lock format version 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlakeLock {
    nodes: BTreeMap<NodeName, LockNode>,
    root: NodeName,
    version: u32,
}

impl FlakeLock {
    /// Deserialize a lock from JSON text.
    pub fn from_json_text(text: &str) -> Result<Self, Error> {
        todo!("serde_json")
    }

    /// Every direct input of the root node that locks a GitHub source, with
    /// its input name and locked source. Topology discovery matches these
    /// against the configured component set by owner and repository.
    pub fn github_inputs(&self) -> Vec<(InputName, LockedSource)> {
        todo!()
    }

    /// Repin the named input in-type, returning the previous locked
    /// revision.
    ///
    /// Sets `locked.rev` to `revision`, `locked.narHash` and
    /// `locked.lastModified` from `prefetched`, and — for a cascade pin —
    /// `original.ref` to the synchronizer branch so a later `nix flake
    /// update` follows the same branch the lock points into.
    pub fn repin_input(
        &mut self,
        input: &InputName,
        reference: BranchName,
        revision: CommitIdentifier,
        prefetched: PrefetchedSource,
    ) -> Result<CommitIdentifier, Error> {
        todo!()
    }

    /// Reserialize in Nix's canonical lock rendering (two-space indent,
    /// object keys sorted).
    pub fn to_json_text(&self) -> Result<String, Error> {
        todo!()
    }
}

/// A node key in the lock's `nodes` table.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeName(String);

/// An input name as declared in `flake.nix` (usually, but not necessarily,
/// equal to the node name).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InputName(String);

impl InputName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One lock node. Only the fields the tool reads or writes are typed; the
/// rest of the node object is preserved through `remainder`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inputs: Option<BTreeMap<InputName, InputReference>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    locked: Option<LockedSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    original: Option<OriginalSource>,
    #[serde(flatten)]
    remainder: serde_json::Map<String, serde_json::Value>,
}

/// How a node's input refers to another node: directly by node name, or by
/// a `follows` path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputReference {
    Direct(NodeName),
    Follows(Vec<NodeName>),
}

/// A locked source (`locked` object) — the exact fetch Nix will perform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedSource {
    owner: String,
    repo: String,
    rev: String,
    #[serde(rename = "narHash")]
    nar_hash: String,
    #[serde(rename = "lastModified")]
    last_modified: u64,
    #[serde(rename = "type")]
    source_type: String,
    #[serde(flatten)]
    remainder: serde_json::Map<String, serde_json::Value>,
}

impl LockedSource {
    /// The repository identity of this source, for matching against the
    /// configured component set.
    pub fn component_name(&self) -> ComponentName {
        todo!()
    }

    pub fn revision(&self) -> CommitIdentifier {
        todo!()
    }
}

/// An original source (`original` object) — what `flake.nix` asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OriginalSource {
    owner: String,
    repo: String,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[serde(rename = "type")]
    source_type: String,
    #[serde(flatten)]
    remainder: serde_json::Map<String, serde_json::Value>,
}

/// A fully pinned flake reference to prefetch, e.g.
/// `github:LiGoldragon/signal-frame/<rev>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedFlakeReference {
    owner: String,
    repository: ComponentName,
    revision: CommitIdentifier,
}

/// What a prefetch yields: the one thing the lock text cannot state from
/// typed knowledge alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchedSource {
    nar_hash: NarHash,
    last_modified: u64,
}

/// The narHash boundary. The single external-command dependency of the
/// flake pin path.
pub trait NarHashSource {
    /// Prefetch `reference` and return its narHash and lastModified.
    fn prefetch(&self, reference: &PinnedFlakeReference) -> Result<PrefetchedSource, Error>;
}

/// The production implementation: `nix flake prefetch --json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NixFlakePrefetch {
    /// The nix binary to invoke, normally `nix` from PATH.
    nix_binary: String,
}

impl NixFlakePrefetch {
    pub fn from_path_environment() -> Self {
        Self {
            nix_binary: "nix".to_string(),
        }
    }
}

impl NarHashSource for NixFlakePrefetch {
    fn prefetch(&self, reference: &PinnedFlakeReference) -> Result<PrefetchedSource, Error> {
        todo!("nix flake prefetch --json github:<owner>/<repository>/<revision>")
    }
}
