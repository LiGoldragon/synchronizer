//! Immutable release-train intent, resolution, and portable Nix projection.
//!
//! A train is deliberately two objects: human-authored NOTA intent may select
//! branches, while resolved closure records contain only immutable commits and
//! attestations. Cargo and flake locks remain component-local projections.

use std::collections::{BTreeMap, BTreeSet};

use blake3::Hasher;
use nota::{NotaDecode, NotaEncode};
use serde::Serialize;

use crate::types::{BranchName, CommitIdentifier, ComponentName, NarHash};

/// A durable name for one release train.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, NotaDecode, NotaEncode, Serialize)]
pub struct ReleaseTrainName(String);

impl ReleaseTrainName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The only branch Synchronizer may materialize for this train.
    pub fn candidate_branch(&self) -> BranchName {
        BranchName::new(format!("train/{}", self.0))
    }
}

/// A train selector is intent, not a build identity.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode, Serialize)]
pub enum CandidateSelector {
    Mainline,
    Branch(BranchName),
    ExactCommit(CommitIdentifier),
}

/// A declared external source that must remain immutable throughout a train.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode, Serialize)]
pub struct ImmutableExternal {
    component: ComponentName,
    commit: CommitIdentifier,
}

impl ImmutableExternal {
    pub fn new(component: ComponentName, commit: CommitIdentifier) -> Self {
        Self { component, commit }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn commit(&self) -> &CommitIdentifier {
        &self.commit
    }
}

/// One component explicitly admitted to a release train.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode, Serialize)]
pub struct TrainComponent {
    component: ComponentName,
    selector: CandidateSelector,
    expected_base: CommitIdentifier,
}

impl TrainComponent {
    pub fn new(
        component: ComponentName,
        selector: CandidateSelector,
        expected_base: CommitIdentifier,
    ) -> Self {
        Self {
            component,
            selector,
            expected_base,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn selector(&self) -> &CandidateSelector {
        &self.selector
    }

    pub fn expected_base(&self) -> &CommitIdentifier {
        &self.expected_base
    }
}

/// The authored NOTA surface at `release-trains/<name>.nota`.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode, Serialize)]
pub struct ReleaseTrainIntent {
    name: ReleaseTrainName,
    components: Vec<TrainComponent>,
    immutable_externals: Vec<ImmutableExternal>,
}

impl ReleaseTrainIntent {
    pub fn new(
        name: ReleaseTrainName,
        components: Vec<TrainComponent>,
        immutable_externals: Vec<ImmutableExternal>,
    ) -> Self {
        Self {
            name,
            components,
            immutable_externals,
        }
    }

    pub fn name(&self) -> &ReleaseTrainName {
        &self.name
    }

    pub fn components(&self) -> &[TrainComponent] {
        &self.components
    }

    pub fn immutable_externals(&self) -> &[ImmutableExternal] {
        &self.immutable_externals
    }

    pub fn to_nota_text(&self) -> String {
        self.to_nota()
    }

    /// Decode the independent authored release-train surface.
    pub fn from_nota_text(text: &str) -> Result<Self, ReleaseTrainError> {
        nota::NotaSource::new(text)
            .parse::<Self>()
            .map_err(|error| ReleaseTrainError::Nota(error.to_string()))
    }

    pub fn component_names(&self) -> BTreeSet<ComponentName> {
        self.components
            .iter()
            .map(|entry| entry.component.clone())
            .collect()
    }

    pub fn admitted_external(&self, component: &ComponentName, commit: &CommitIdentifier) -> bool {
        self.immutable_externals
            .iter()
            .any(|external| external.component() == component && external.commit() == commit)
    }
}

/// A branch selector observed at one immutable commit and its declared base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSelector {
    component: ComponentName,
    selected: CommitIdentifier,
    observed_base: CommitIdentifier,
    candidate: CommitIdentifier,
}

impl ResolvedSelector {
    pub fn new(
        component: ComponentName,
        selected: CommitIdentifier,
        observed_base: CommitIdentifier,
        candidate: CommitIdentifier,
    ) -> Self {
        Self {
            component,
            selected,
            observed_base,
            candidate,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn selected(&self) -> &CommitIdentifier {
        &self.selected
    }

    pub fn observed_base(&self) -> &CommitIdentifier {
        &self.observed_base
    }

    pub fn candidate(&self) -> &CommitIdentifier {
        &self.candidate
    }
}

/// Source proof needed by the portable Nix integration projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NixSourceAttestation {
    component: ComponentName,
    commit: CommitIdentifier,
    nar_hash: NarHash,
}

impl NixSourceAttestation {
    pub fn new(component: ComponentName, commit: CommitIdentifier, nar_hash: NarHash) -> Self {
        Self {
            component,
            commit,
            nar_hash,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn commit(&self) -> &CommitIdentifier {
        &self.commit
    }

    pub fn nar_hash(&self) -> &NarHash {
        &self.nar_hash
    }
}

/// A component-local lock projection. It deliberately cannot be shared across
/// consumers: Cargo resolves each manifest graph independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComponentLockIdentity {
    component: ComponentName,
    cargo_lock_blake3: String,
    flake_lock_blake3: String,
}

impl ComponentLockIdentity {
    pub fn from_text(component: ComponentName, cargo_lock: &str, flake_lock: &str) -> Self {
        Self {
            component,
            cargo_lock_blake3: Self::content_hash(cargo_lock),
            flake_lock_blake3: Self::content_hash(flake_lock),
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    fn content_hash(text: &str) -> String {
        blake3::hash(text.as_bytes()).to_hex().to_string()
    }
}

/// A content-addressed source reuse seam. It is intentionally data only;
/// materialization and compiled-output indexing remain later measured work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VendorSnapshotReference {
    identity: String,
}

impl VendorSnapshotReference {
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
        }
    }
}

/// Immutable, builder-consumable release-train closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedReleaseTrain {
    name: ReleaseTrainName,
    candidate_branch: BranchName,
    components: Vec<ResolvedComponent>,
    attestations: Vec<NixSourceAttestation>,
    locks: Vec<ComponentLockIdentity>,
    vendor_snapshot: Option<VendorSnapshotReference>,
    identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedComponent {
    component: ComponentName,
    selected_commit: CommitIdentifier,
    candidate_commit: CommitIdentifier,
}

impl ResolvedComponent {
    pub fn from_selector(selector: &ResolvedSelector) -> Self {
        Self {
            component: selector.component().clone(),
            selected_commit: selector.selected().clone(),
            candidate_commit: selector.candidate().clone(),
        }
    }
}

/// Data-bearing resolver input; remote/ref discovery belongs at the caller's
/// Git boundary and this object makes all post-discovery invariants testable.
pub struct ReleaseTrainResolution {
    intent: ReleaseTrainIntent,
    selectors: Vec<ResolvedSelector>,
    attestations: Vec<NixSourceAttestation>,
    locks: Vec<ComponentLockIdentity>,
    discovered_internal_components: BTreeSet<ComponentName>,
    discovered_external_components: BTreeMap<ComponentName, CommitIdentifier>,
}

impl ReleaseTrainResolution {
    pub fn new(
        intent: ReleaseTrainIntent,
        selectors: Vec<ResolvedSelector>,
        attestations: Vec<NixSourceAttestation>,
        locks: Vec<ComponentLockIdentity>,
        discovered_internal_components: BTreeSet<ComponentName>,
        discovered_external_components: BTreeMap<ComponentName, CommitIdentifier>,
    ) -> Self {
        Self {
            intent,
            selectors,
            attestations,
            locks,
            discovered_internal_components,
            discovered_external_components,
        }
    }

    /// Validates declared boundaries before emitting an immutable closure.
    pub fn resolve(self) -> Result<ResolvedReleaseTrain, ReleaseTrainError> {
        self.validate_membership()?;
        self.validate_selectors()?;
        self.validate_attestations()?;
        let mut components: Vec<ResolvedComponent> = self
            .selectors
            .iter()
            .map(ResolvedComponent::from_selector)
            .collect();
        components.sort_by(|left, right| left.component.as_str().cmp(right.component.as_str()));
        let mut attestations = self.attestations;
        attestations.sort_by(|left, right| left.component.as_str().cmp(right.component.as_str()));
        let mut locks = self.locks;
        locks.sort_by(|left, right| left.component.as_str().cmp(right.component.as_str()));
        let mut resolved = ResolvedReleaseTrain {
            name: self.intent.name.clone(),
            candidate_branch: self.intent.name.candidate_branch(),
            components,
            attestations,
            locks,
            vendor_snapshot: None,
            identity: String::new(),
        };
        resolved.identity = resolved.payload_identity();
        Ok(resolved)
    }

    fn validate_membership(&self) -> Result<(), ReleaseTrainError> {
        let declared = self.intent.component_names();
        for discovered in &self.discovered_internal_components {
            if !declared.contains(discovered) {
                return Err(ReleaseTrainError::UndeclaredInternalEdge(
                    discovered.clone(),
                ));
            }
        }
        for declared_component in declared {
            if !self
                .discovered_internal_components
                .contains(&declared_component)
            {
                return Err(ReleaseTrainError::MissingDeclaredComponent(
                    declared_component,
                ));
            }
        }
        for (external, commit) in &self.discovered_external_components {
            if !self.intent.admitted_external(external, commit) {
                return Err(ReleaseTrainError::UnadmittedExternal {
                    component: external.clone(),
                    commit: commit.clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_selectors(&self) -> Result<(), ReleaseTrainError> {
        for declared in self.intent.components() {
            let Some(observed) = self
                .selectors
                .iter()
                .find(|selector| selector.component() == declared.component())
            else {
                return Err(ReleaseTrainError::MissingResolution(
                    declared.component().clone(),
                ));
            };
            if observed.observed_base() != declared.expected_base() {
                return Err(ReleaseTrainError::ExpectedBaseMoved {
                    component: declared.component().clone(),
                    expected: declared.expected_base().clone(),
                    observed: observed.observed_base().clone(),
                });
            }
            if let CandidateSelector::ExactCommit(commit) = declared.selector()
                && observed.selected() != commit
            {
                return Err(ReleaseTrainError::ExactSelectorMoved {
                    component: declared.component().clone(),
                    expected: commit.clone(),
                    observed: observed.selected().clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_attestations(&self) -> Result<(), ReleaseTrainError> {
        for selector in &self.selectors {
            let Some(attestation) = self
                .attestations
                .iter()
                .find(|attestation| attestation.component() == selector.component())
            else {
                return Err(ReleaseTrainError::MissingAttestation(
                    selector.component().clone(),
                ));
            };
            if attestation.commit() != selector.candidate() {
                return Err(ReleaseTrainError::AttestationCommitMismatch {
                    component: selector.component().clone(),
                    candidate: selector.candidate().clone(),
                    attested: attestation.commit().clone(),
                });
            }
        }
        Ok(())
    }
}

impl ResolvedReleaseTrain {
    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn candidate_branch(&self) -> &BranchName {
        &self.candidate_branch
    }

    pub fn components(&self) -> &[ResolvedComponent] {
        &self.components
    }

    /// Canonical bootstrap JSON for `builtins.fromJSON`; a future TextualJson
    /// implementation owns the same projection boundary without changing the
    /// typed closure or its domain identity.
    pub fn to_canonical_json(&self) -> Result<String, ReleaseTrainError> {
        serde_json::to_string(self).map_err(|error| ReleaseTrainError::Json(error.to_string()))
    }

    /// A portable integration flake which fetches only commit/narHash pairs.
    pub fn to_integration_flake(&self, owner: &str) -> String {
        let inputs = self
            .attestations
            .iter()
            .map(|source| {
                format!(
                    "    {} = {{ url = \"github:{}/{}/{}\"; narHash = \"{}\"; }};",
                    source.component().as_str().replace('-', "_"),
                    owner,
                    source.component().as_str(),
                    source.commit().as_str(),
                    source.nar_hash().as_str(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{{\n  description = \"release train {} ({})\";\n  inputs = {{\n{}\n  }};\n  outputs = inputs: {{\n    releaseTrain = builtins.fromJSON (builtins.readFile ./release-train.lock.json);\n  }};\n}}\n",
            self.name.as_str(),
            self.identity,
            inputs
        )
    }

    fn payload_identity(&self) -> String {
        let payload = serde_json::to_vec(&ResolvedTrainIdentityPayload::from(self))
            .expect("resolved release train identity payload is serializable");
        let mut hasher = Hasher::new();
        hasher.update(b"LiGoldragon.release-train.resolved.v1\0");
        hasher.update(&payload);
        hasher.finalize().to_hex().to_string()
    }
}

#[derive(Serialize)]
struct ResolvedTrainIdentityPayload<'a> {
    name: &'a ReleaseTrainName,
    candidate_branch: &'a BranchName,
    components: &'a [ResolvedComponent],
    attestations: &'a [NixSourceAttestation],
    locks: &'a [ComponentLockIdentity],
    vendor_snapshot: &'a Option<VendorSnapshotReference>,
}

impl<'a> From<&'a ResolvedReleaseTrain> for ResolvedTrainIdentityPayload<'a> {
    fn from(train: &'a ResolvedReleaseTrain) -> Self {
        Self {
            name: &train.name,
            candidate_branch: &train.candidate_branch,
            components: &train.components,
            attestations: &train.attestations,
            locks: &train.locks,
            vendor_snapshot: &train.vendor_snapshot,
        }
    }
}

/// Explicit loud train failures; callers never silently rewrite discovered
/// topology to match authored intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseTrainError {
    UndeclaredInternalEdge(ComponentName),
    MissingDeclaredComponent(ComponentName),
    UnadmittedExternal {
        component: ComponentName,
        commit: CommitIdentifier,
    },
    MissingResolution(ComponentName),
    ExpectedBaseMoved {
        component: ComponentName,
        expected: CommitIdentifier,
        observed: CommitIdentifier,
    },
    ExactSelectorMoved {
        component: ComponentName,
        expected: CommitIdentifier,
        observed: CommitIdentifier,
    },
    MissingAttestation(ComponentName),
    AttestationCommitMismatch {
        component: ComponentName,
        candidate: CommitIdentifier,
        attested: CommitIdentifier,
    },
    Json(String),
    Nota(String),
}

impl std::fmt::Display for ReleaseTrainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UndeclaredInternalEdge(component) => write!(
                formatter,
                "undeclared internal component edge: {}",
                component.as_str()
            ),
            Self::MissingDeclaredComponent(component) => write!(
                formatter,
                "declared component absent from discovered topology: {}",
                component.as_str()
            ),
            Self::UnadmittedExternal { component, commit } => write!(
                formatter,
                "external component is not admitted immutably: {}@{}",
                component.as_str(),
                commit.as_str()
            ),
            Self::MissingResolution(component) => write!(
                formatter,
                "component has no resolved selector: {}",
                component.as_str()
            ),
            Self::ExpectedBaseMoved {
                component,
                expected,
                observed,
            } => write!(
                formatter,
                "expected base moved for {}: expected {}, observed {}",
                component.as_str(),
                expected.as_str(),
                observed.as_str()
            ),
            Self::ExactSelectorMoved {
                component,
                expected,
                observed,
            } => write!(
                formatter,
                "exact selector moved for {}: expected {}, observed {}",
                component.as_str(),
                expected.as_str(),
                observed.as_str()
            ),
            Self::MissingAttestation(component) => write!(
                formatter,
                "component has no Nix source attestation: {}",
                component.as_str()
            ),
            Self::AttestationCommitMismatch {
                component,
                candidate,
                attested,
            } => write!(
                formatter,
                "Nix attestation mismatch for {}: candidate {}, attested {}",
                component.as_str(),
                candidate.as_str(),
                attested.as_str()
            ),
            Self::Json(detail) => write!(formatter, "canonical JSON projection failed: {detail}"),
            Self::Nota(detail) => write!(
                formatter,
                "release-train intent NOTA decode failed: {detail}"
            ),
        }
    }
}

impl std::error::Error for ReleaseTrainError {}
