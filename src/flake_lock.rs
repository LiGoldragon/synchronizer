//! Typed model of a component's `flake.lock`.
//!
//! The lock is JSON, deserialized with serde into typed data. Repinning an
//! input happens in-type: the new revision is set on the locked node, the
//! narHash is obtained through the [`NarHashSource`] boundary — the *only*
//! external command in the pin-editing path — and the document is
//! reserialized in Nix's own lock rendering (two-space indent, object keys
//! in Nix's alphabetical order, trailing newline).
//!
//! Field declarations are alphabetical because serde serializes in
//! declaration order and Nix writes keys sorted; known keys of every lock
//! source shape are modeled explicitly so untouched nodes reserialize
//! byte-identically. Unknown future keys survive through `remainder` but
//! append after the modeled keys — a documented fidelity caveat, not a
//! correctness one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::topology::PinLayer;
use crate::types::{CommitIdentifier, ComponentName, NarHash};

/// A `flake.lock` document (lock format version 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlakeLock {
    nodes: BTreeMap<NodeName, LockNode>,
    root: NodeName,
    version: u32,
}

impl FlakeLock {
    /// Deserialize a lock from JSON text.
    pub fn from_json_text(text: &str, component: &ComponentName) -> Result<Self, Error> {
        serde_json::from_str(text).map_err(|error| Error::ManifestDecode {
            component: component.clone(),
            layer: PinLayer::FlakeLock,
            detail: error.to_string(),
        })
    }

    /// Every direct input of the root node that locks a GitHub source, with
    /// its input name and locked source. Topology discovery matches these
    /// against the configured component set by owner and repository.
    /// `follows` references are not direct pins of this lock and produce
    /// nothing.
    pub fn github_inputs(&self) -> Vec<(InputName, &LockedSource)> {
        let Some(root) = self.nodes.get(&self.root) else {
            return Vec::new();
        };
        let Some(inputs) = &root.inputs else {
            return Vec::new();
        };
        inputs
            .iter()
            .filter_map(|(input_name, reference)| {
                let InputReference::Direct(node_name) = reference else {
                    return None;
                };
                let node = self.nodes.get(node_name)?;
                let locked = node.locked.as_ref()?;
                locked.is_github().then(|| (input_name.clone(), locked))
            })
            .collect()
    }

    /// Repin the named root input in-type, returning the previous locked
    /// revision.
    ///
    /// Sets `locked.rev`, `locked.narHash`, and `locked.lastModified` from
    /// `prefetched`. The node's `original` is **always preserved** — on a
    /// cascade pin too. The locked `rev` alone carries the cascade: Nix
    /// re-resolves originals from `flake.nix` on update, so a lock whose
    /// `original` mismatches what `flake.nix` declares is discarded and the
    /// input re-locked from the declaration (back to `main`, reintroducing
    /// the skew this tool exists to kill; proven against Nix 2.34.6).
    pub fn repin_input(
        &mut self,
        consumer: &ComponentName,
        input: &InputName,
        revision: CommitIdentifier,
        prefetched: PrefetchedSource,
    ) -> Result<CommitIdentifier, Error> {
        let node_name = self
            .nodes
            .get(&self.root)
            .and_then(|root| root.inputs.as_ref())
            .and_then(|inputs| inputs.get(input))
            .and_then(|reference| match reference {
                InputReference::Direct(node_name) => Some(node_name.clone()),
                InputReference::Follows(_) => None,
            })
            .ok_or_else(|| Error::ManifestEncode {
                component: consumer.clone(),
                layer: PinLayer::FlakeLock,
                detail: format!("no direct root input named {}", input.as_str()),
            })?;
        let node = self
            .nodes
            .get_mut(&node_name)
            .ok_or_else(|| Error::ManifestEncode {
                component: consumer.clone(),
                layer: PinLayer::FlakeLock,
                detail: format!("lock node {} missing", node_name.as_str()),
            })?;
        let locked = node.locked.as_mut().ok_or_else(|| Error::ManifestEncode {
            component: consumer.clone(),
            layer: PinLayer::FlakeLock,
            detail: format!("lock node {} carries no locked source", node_name.as_str()),
        })?;
        let previous = CommitIdentifier::new(locked.rev.clone().unwrap_or_default());
        locked.rev = Some(revision.as_str().to_string());
        locked.nar_hash = Some(prefetched.nar_hash.as_str().to_string());
        locked.last_modified = Some(prefetched.last_modified);
        Ok(previous)
    }

    /// Reserialize in Nix's canonical lock rendering.
    pub fn to_json_text(&self) -> Result<String, Error> {
        let rendered =
            serde_json::to_string_pretty(self).map_err(|error| Error::ManifestEncode {
                component: ComponentName::new("flake.lock"),
                layer: PinLayer::FlakeLock,
                detail: error.to_string(),
            })?;
        Ok(format!("{rendered}\n"))
    }
}

/// A node key in the lock's `nodes` table.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeName(String);

impl NodeName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An input name as declared in `flake.nix` (usually, but not necessarily,
/// equal to the node name).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InputName(String);

impl InputName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One lock node. Known fields are typed in Nix's alphabetical key order;
/// the rest of the node object survives through `remainder`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flake: Option<bool>,
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
/// Fields cover the github, git, tarball, and path source shapes; only
/// `type` is universal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dir: Option<String>,
    #[serde(
        default,
        rename = "lastModified",
        skip_serializing_if = "Option::is_none"
    )]
    last_modified: Option<u64>,
    #[serde(default, rename = "narHash", skip_serializing_if = "Option::is_none")]
    nar_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rev: Option<String>,
    #[serde(default, rename = "revCount", skip_serializing_if = "Option::is_none")]
    rev_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    submodules: Option<bool>,
    #[serde(rename = "type")]
    source_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(flatten)]
    remainder: serde_json::Map<String, serde_json::Value>,
}

impl LockedSource {
    pub fn is_github(&self) -> bool {
        self.source_type == "github" && self.owner.is_some() && self.repo.is_some()
    }

    /// The forge owner of a GitHub source.
    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    /// The repository identity of this source, for matching against the
    /// configured component set.
    pub fn component_name(&self) -> Option<ComponentName> {
        self.repo.as_deref().map(ComponentName::new)
    }

    pub fn revision(&self) -> Option<CommitIdentifier> {
        self.rev.as_deref().map(CommitIdentifier::new)
    }
}

/// An original source (`original` object) — what `flake.nix` asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OriginalSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    submodules: Option<bool>,
    #[serde(rename = "type")]
    source_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(flatten)]
    remainder: serde_json::Map<String, serde_json::Value>,
}

impl OriginalSource {
    pub fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }
}

/// A fully pinned flake reference to prefetch, e.g.
/// `github:LiGoldragon/signal-frame/<rev>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedFlakeReference {
    owner: String,
    repository: ComponentName,
    revision: CommitIdentifier,
}

impl PinnedFlakeReference {
    pub fn new(
        owner: impl Into<String>,
        repository: ComponentName,
        revision: CommitIdentifier,
    ) -> Self {
        Self {
            owner: owner.into(),
            repository,
            revision,
        }
    }

    /// The `github:` flake reference this pin addresses.
    pub fn to_flake_reference(&self) -> crate::types::FlakeReference {
        crate::types::FlakeReference::new(format!(
            "github:{}/{}/{}",
            self.owner,
            self.repository.as_str(),
            self.revision.as_str()
        ))
    }
}

/// What a prefetch yields: the one thing the lock text cannot state from
/// typed knowledge alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchedSource {
    nar_hash: NarHash,
    last_modified: u64,
}

impl PrefetchedSource {
    pub fn new(nar_hash: NarHash, last_modified: u64) -> Self {
        Self {
            nar_hash,
            last_modified,
        }
    }
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
        let flake_reference = reference.to_flake_reference();
        let output = std::process::Command::new(&self.nix_binary)
            .args(["flake", "prefetch", "--json", flake_reference.as_str()])
            .output()
            .map_err(|error| Error::NarHashPrefetch {
                reference: flake_reference.clone(),
                detail: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(Error::NarHashPrefetch {
                reference: flake_reference,
                detail: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        let decoded: PrefetchReply =
            serde_json::from_slice(&output.stdout).map_err(|error| Error::NarHashPrefetch {
                reference: flake_reference.clone(),
                detail: format!("prefetch reply undecodable: {error}"),
            })?;
        Ok(PrefetchedSource {
            nar_hash: NarHash::new(decoded.hash),
            last_modified: decoded.locked.last_modified,
        })
    }
}

/// The JSON reply shape of `nix flake prefetch --json`.
#[derive(Debug, Deserialize)]
struct PrefetchReply {
    hash: String,
    locked: PrefetchReplyLocked,
}

#[derive(Debug, Deserialize)]
struct PrefetchReplyLocked {
    #[serde(rename = "lastModified")]
    last_modified: u64,
}
