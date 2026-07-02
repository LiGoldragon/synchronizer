//! Cascade-aware version resolution.
//!
//! The rule (locked design, ARCHITECTURE.md §6): a dependency that was not
//! bumped this run targets the latest pushed `main` tip, queried read-only
//! from the remote; a dependency that *was* bumped this run targets its
//! pushed `synchronizer` branch tip instead, so consumers pick up the
//! cascade rather than a `main` that does not carry it yet. A failed verify
//! does not remove a component from the ledger: consumers still pin the
//! pushed synchronizer tip, and their own verifies report the breakage
//! (§7 of the locked design accepts broken synchronizer branches).

use std::collections::BTreeMap;

use crate::component_manifests::ComponentManifests;
use crate::error::Error;
use crate::topology::DependencyEdge;
use crate::types::{CommitIdentifier, ComponentName};

/// Where a dependency's target revision comes from this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTarget {
    /// Not bumped this run: the latest pushed `main` tip.
    RemoteMainTip(CommitIdentifier),
    /// Bumped this run: the `synchronizer` branch tip this run pushed.
    SynchronizerTip(CommitIdentifier),
}

impl ResolvedTarget {
    pub fn revision(&self) -> &CommitIdentifier {
        match self {
            Self::RemoteMainTip(revision) => revision,
            Self::SynchronizerTip(revision) => revision,
        }
    }
}

/// The components bumped so far this run, with the synchronizer tips this
/// run pushed for them. Grows monotonically during the ascent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BumpLedger(BTreeMap<ComponentName, CommitIdentifier>);

impl BumpLedger {
    pub fn record(&mut self, component: ComponentName, synchronizer_tip: CommitIdentifier) {
        todo!()
    }

    pub fn synchronizer_tip_of(&self, component: &ComponentName) -> Option<&CommitIdentifier> {
        todo!()
    }
}

/// Resolves dependency targets and detects stale pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionResolver {
    /// Remote `main` tips, one per configured component, queried once at
    /// run start.
    main_tips: BTreeMap<ComponentName, CommitIdentifier>,
    ledger: BumpLedger,
}

impl VersionResolver {
    pub fn new(main_tips: BTreeMap<ComponentName, CommitIdentifier>) -> Self {
        todo!()
    }

    /// The cascade rule: ledger hit means `SynchronizerTip`, otherwise
    /// `RemoteMainTip`.
    pub fn resolve(&self, component: &ComponentName) -> Result<ResolvedTarget, Error> {
        todo!()
    }

    /// Record a bump so later consumers resolve to the pushed tip.
    pub fn record_bump(&mut self, component: ComponentName, synchronizer_tip: CommitIdentifier) {
        todo!()
    }

    /// Every stale pin of `consumer`: edges whose pinned revision differs
    /// from the resolved target revision.
    pub fn stale_pins(
        &self,
        consumer: &ComponentManifests,
        edges: &[&DependencyEdge],
    ) -> Result<Vec<StalePin>, Error> {
        todo!()
    }
}

/// One pin that must move: the edge, what it currently pins, and where it
/// must go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePin {
    edge: DependencyEdge,
    pinned: CommitIdentifier,
    target: ResolvedTarget,
}

impl StalePin {
    pub fn edge(&self) -> &DependencyEdge {
        &self.edge
    }

    pub fn pinned(&self) -> &CommitIdentifier {
        &self.pinned
    }

    pub fn target(&self) -> &ResolvedTarget {
        &self.target
    }
}
