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
use crate::report::PinValue;
use crate::topology::{DependencyEdge, PinLayer};
use crate::types::{BranchName, CommitIdentifier, ComponentName};

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

    /// The branch a consumer's manifest-layer declaration must follow to
    /// reach this target from a fresh clone.
    pub fn reachable_branch(&self) -> BranchName {
        match self {
            Self::RemoteMainTip(_) => BranchName::main(),
            Self::SynchronizerTip(_) => BranchName::synchronizer(),
        }
    }
}

/// The components bumped so far this run, with the synchronizer tips this
/// run pushed for them. Grows monotonically during the ascent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BumpLedger(BTreeMap<ComponentName, CommitIdentifier>);

impl BumpLedger {
    pub fn record(&mut self, component: ComponentName, synchronizer_tip: CommitIdentifier) {
        self.0.insert(component, synchronizer_tip);
    }

    pub fn synchronizer_tip_of(&self, component: &ComponentName) -> Option<&CommitIdentifier> {
        self.0.get(component)
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
        Self {
            main_tips,
            ledger: BumpLedger::default(),
        }
    }

    /// The cascade rule: ledger hit means `SynchronizerTip`, otherwise
    /// `RemoteMainTip`.
    pub fn resolve(&self, component: &ComponentName) -> Result<ResolvedTarget, Error> {
        if let Some(tip) = self.ledger.synchronizer_tip_of(component) {
            return Ok(ResolvedTarget::SynchronizerTip(tip.clone()));
        }
        self.main_tips
            .get(component)
            .map(|tip| ResolvedTarget::RemoteMainTip(tip.clone()))
            .ok_or_else(|| Error::UnknownComponent(component.clone()))
    }

    /// Record a bump so later consumers resolve to the pushed tip.
    pub fn record_bump(&mut self, component: ComponentName, synchronizer_tip: CommitIdentifier) {
        self.ledger.record(component, synchronizer_tip);
    }

    /// Every stale pin of `consumer`: lock-layer and URL-pin edges whose
    /// pinned revision differs from the resolved target revision, and
    /// Cargo-manifest edges whose declared branch cannot reach a cascade
    /// target (`branch = "main"` while the target is a synchronizer tip).
    pub fn stale_pins(
        &self,
        consumer: &ComponentManifests,
        edges: &[&DependencyEdge],
    ) -> Result<Vec<StalePin>, Error> {
        let mut stale = Vec::new();
        for edge in edges {
            let target = self.resolve(edge.producer())?;
            let pinned = consumer.pinned_value(edge)?;
            let is_stale = match (edge.layer(), &pinned) {
                (PinLayer::CargoLock | PinLayer::FlakeLock, PinValue::Revision(revision)) => {
                    revision != target.revision()
                }
                (PinLayer::FlakeManifest, PinValue::Revision(revision)) => {
                    revision != target.revision()
                }
                (PinLayer::CargoManifest, PinValue::Reference(branch)) => {
                    matches!(target, ResolvedTarget::SynchronizerTip(_))
                        && branch != &target.reachable_branch()
                }
                // A revision-pinned manifest declaration moves with the lock
                // rule; a reference-shaped lock pin cannot occur.
                (PinLayer::CargoManifest, PinValue::Revision(revision)) => {
                    revision != target.revision()
                }
                (_, PinValue::Reference(_)) => false,
            };
            if is_stale {
                stale.push(StalePin {
                    edge: (*edge).clone(),
                    pinned,
                    target,
                });
            }
        }
        Ok(stale)
    }
}

/// One pin that must move: the edge, what it currently pins, and where it
/// must go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePin {
    edge: DependencyEdge,
    pinned: PinValue,
    target: ResolvedTarget,
}

impl StalePin {
    pub fn new(edge: DependencyEdge, pinned: PinValue, target: ResolvedTarget) -> Self {
        Self {
            edge,
            pinned,
            target,
        }
    }

    pub fn edge(&self) -> &DependencyEdge {
        &self.edge
    }

    pub fn pinned(&self) -> &PinValue {
        &self.pinned
    }

    pub fn target(&self) -> &ResolvedTarget {
        &self.target
    }
}
