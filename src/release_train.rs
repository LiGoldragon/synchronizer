//! Immutable release-train intent, resolution, and portable Nix projection.
//!
//! A train is deliberately two objects: human-authored NOTA intent may select
//! branches, while resolved closure records contain only immutable commits and
//! attestations. Cargo and flake locks remain component-local projections.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use blake3::Hasher;
use nota::{NotaDecode, NotaEncode};
use serde::Serialize;

use crate::component_manifests::ComponentManifests;
use crate::configuration::SynchronizerConfig;
use crate::driver::{BaseSelection, RunBoundaries, SynchronizerRun};
use crate::flake_lock::PinnedFlakeReference;
use crate::git_repository::{CommitMessage, ComponentRepository};
use crate::report::Action;
use crate::topology::DependencyGraph;
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
    last_modified: u64,
}

impl NixSourceAttestation {
    pub fn new(component: ComponentName, commit: CommitIdentifier, nar_hash: NarHash) -> Self {
        Self {
            component,
            commit,
            nar_hash,
            last_modified: 0,
        }
    }

    pub fn with_last_modified(mut self, last_modified: u64) -> Self {
        self.last_modified = last_modified;
        self
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

    pub fn last_modified(&self) -> u64 {
        self.last_modified
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

    /// A portable integration flake. Its source hashes belong in the emitted
    /// `flake.lock`, not beside `inputs.<name>.url`; Nix validates the locked
    /// fixed source before evaluating that component's own flake.
    pub fn to_integration_flake(&self, owner: &str) -> String {
        let inputs = self
            .attestations
            .iter()
            .map(|source| {
                format!(
                    "    {}.url = \"github:{}/{}/{}\";",
                    source.component().as_str().replace('-', "_"),
                    owner,
                    source.component().as_str(),
                    source.commit().as_str(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{{\n  description = \"release train {} ({})\";\n  inputs = {{\n{}\n  }};\n  outputs = inputs:\n    let\n      systems = [ \"aarch64-darwin\" \"aarch64-linux\" \"x86_64-darwin\" \"x86_64-linux\" ];\n      releaseTrain = builtins.fromJSON (builtins.readFile ./release-train.lock.json);\n      components = builtins.removeAttrs inputs [ \"self\" ];\n      componentPackages = system: builtins.mapAttrs (_: component: component.packages.${{system}}.default) components;\n      perSystem = builtins.listToAttrs (map (system: {{ name = system; value = componentPackages system; }}) systems);\n    in {{\n      inherit releaseTrain;\n      packages = perSystem;\n      checks = perSystem;\n    }};\n}}\n",
            self.name.as_str(),
            self.identity,
            inputs
        )
    }

    /// A Nix flake lock is the authoritative placement of fixed-source hashes
    /// for the generated integration flake.
    pub fn to_integration_flake_lock(&self, owner: &str) -> Result<String, ReleaseTrainError> {
        let mut nodes = serde_json::Map::new();
        let mut root_inputs = serde_json::Map::new();
        for source in &self.attestations {
            let name = source.component().as_str().replace('-', "_");
            root_inputs.insert(name.clone(), serde_json::Value::String(name.clone()));
            nodes.insert(name, serde_json::json!({
                "locked": { "lastModified": source.last_modified(), "narHash": source.nar_hash().as_str(), "owner": owner, "repo": source.component().as_str(), "rev": source.commit().as_str(), "type": "github" },
                "original": { "owner": owner, "repo": source.component().as_str(), "type": "github" }
            }));
        }
        nodes.insert(
            "root".to_string(),
            serde_json::json!({ "inputs": root_inputs }),
        );
        serde_json::to_string_pretty(
            &serde_json::json!({ "nodes": nodes, "root": "root", "version": 7 }),
        )
        .map(|text| format!("{text}\n"))
        .map_err(|error| ReleaseTrainError::Json(error.to_string()))
    }

    /// Emit the P2 portable integration inputs. The files contain no local
    /// source reference; the caller chooses only their output directory.
    pub fn write_integration_artifacts(
        &self,
        output_directory: &Path,
        owner: &str,
    ) -> Result<ReleaseTrainArtifacts, ReleaseTrainError> {
        std::fs::create_dir_all(output_directory)
            .map_err(|error| ReleaseTrainError::Io(error.to_string()))?;
        let json_path = output_directory.join("release-train.lock.json");
        let flake_path = output_directory.join("flake.nix");
        let flake_lock_path = output_directory.join("flake.lock");
        std::fs::write(&json_path, self.to_canonical_json()?)
            .map_err(|error| ReleaseTrainError::Io(error.to_string()))?;
        std::fs::write(&flake_path, self.to_integration_flake(owner))
            .map_err(|error| ReleaseTrainError::Io(error.to_string()))?;
        std::fs::write(&flake_lock_path, self.to_integration_flake_lock(owner)?)
            .map_err(|error| ReleaseTrainError::Io(error.to_string()))?;
        Ok(ReleaseTrainArtifacts::new(
            json_path,
            flake_path,
            flake_lock_path,
        ))
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

/// Paths of generated P2 artifacts; their contents remain portable closure
/// projections and not another lock authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTrainArtifacts {
    json_path: PathBuf,
    flake_path: PathBuf,
    flake_lock_path: PathBuf,
}

impl ReleaseTrainArtifacts {
    pub fn new(json_path: PathBuf, flake_path: PathBuf, flake_lock_path: PathBuf) -> Self {
        Self {
            json_path,
            flake_path,
            flake_lock_path,
        }
    }

    pub fn json_path(&self) -> &Path {
        &self.json_path
    }

    pub fn flake_path(&self) -> &Path {
        &self.flake_path
    }

    pub fn flake_lock_path(&self) -> &Path {
        &self.flake_lock_path
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

/// A live release-train operation. It resolves only pushed selectors, creates
/// per-component `train/<name>` bootstrap commits, then reuses the ordinary
/// typed Cargo/flake cascade against those candidate branches.
pub struct ReleaseTrainRun {
    config: SynchronizerConfig,
    intent: ReleaseTrainIntent,
    boundaries: RunBoundaries,
}

impl ReleaseTrainRun {
    /// Construct the production release-train operation. Tests inject
    /// `RunBoundaries` through [`Self::new`] so no fixture reaches a remote.
    pub fn from_config(config: SynchronizerConfig, intent: ReleaseTrainIntent) -> Self {
        let boundaries = RunBoundaries::from_config(&config);
        Self::new(config, intent, boundaries)
    }

    pub fn new(
        config: SynchronizerConfig,
        intent: ReleaseTrainIntent,
        boundaries: RunBoundaries,
    ) -> Self {
        Self {
            config,
            intent,
            boundaries,
        }
    }

    /// Resolve selectors, prove expected bases, and materialize isolated train
    /// branches before cascading ordinary per-consumer Cargo/flake lock edits.
    pub fn execute(mut self) -> Result<MaterializedReleaseTrain, ReleaseTrainError> {
        let candidate_branch = self.intent.name().candidate_branch();
        let component_names = self
            .intent
            .components()
            .iter()
            .map(|component| component.component().clone())
            .collect::<Vec<_>>();
        let mut selectors = Vec::new();
        for component in self.intent.components() {
            let (selected, observed_base) = self.resolve_selector(component)?;
            let candidate = self.materialize_candidate(component, &selected, &candidate_branch)?;
            selectors.push(ResolvedSelector::new(
                component.component().clone(),
                selected,
                observed_base,
                candidate,
            ));
        }
        let train_config = self
            .config
            .release_train_view(&component_names, candidate_branch.clone())
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        let (report, boundaries) = SynchronizerRun::with_boundaries(train_config, self.boundaries)
            .with_base_selection(BaseSelection::StagedCascade)
            .execute_with_boundaries()
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        self.boundaries = boundaries;
        for level in report.levels() {
            for outcome in level.repositories() {
                if let Action::Bumped(bump) = outcome.action()
                    && let Some(selector) = selectors
                        .iter_mut()
                        .find(|selector| selector.component() == outcome.component())
                {
                    selector.candidate = bump.pushed().tip().clone();
                }
            }
        }
        let closure = self.resolve_candidate_closure(&selectors)?;
        Ok(MaterializedReleaseTrain {
            intent: self.intent,
            candidate_branch,
            selectors,
            report,
            closure,
        })
    }

    fn resolve_selector(
        &self,
        component: &TrainComponent,
    ) -> Result<(CommitIdentifier, CommitIdentifier), ReleaseTrainError> {
        let repository = self.open_component(component.component())?;
        let selected = match component.selector() {
            CandidateSelector::Mainline => repository
                .remote_main_tip()
                .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?,
            CandidateSelector::Branch(branch) => repository
                .remote_branch_tip(branch)
                .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?
                .ok_or_else(|| ReleaseTrainError::MissingSelectedBranch {
                    component: component.component().clone(),
                    branch: branch.clone(),
                })?,
            CandidateSelector::ExactCommit(commit) => commit.clone(),
        };
        // Observe the remote mainline independently.  This is the value the
        // closure records as its actual base; it must never be reconstructed
        // from the authored expectation.
        let observed_base = repository
            .remote_main_tip()
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        if &observed_base != component.expected_base() {
            return Err(ReleaseTrainError::ExpectedBaseMoved {
                component: component.component().clone(),
                expected: component.expected_base().clone(),
                observed: observed_base,
            });
        }
        repository
            .fetch(&selected)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        let base_matches = repository
            .base_is_ancestor(component.expected_base(), &selected)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        if !base_matches {
            return Err(ReleaseTrainError::ExpectedBaseMoved {
                component: component.component().clone(),
                expected: component.expected_base().clone(),
                observed: selected,
            });
        }
        Ok((selected, observed_base))
    }

    /// Gather the evidence from the actual candidate commits after the cascade
    /// has finished, then make every closure validator part of the normal path.
    fn resolve_candidate_closure(
        &self,
        selectors: &[ResolvedSelector],
    ) -> Result<ResolvedReleaseTrain, ReleaseTrainError> {
        let mut manifests = Vec::new();
        let mut attestations = Vec::new();
        let mut locks = Vec::new();
        for selector in selectors {
            let repository = self.open_component(selector.component())?;
            repository
                .fetch(selector.candidate())
                .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
            let manifests_at_candidate = ComponentManifests::load_at(
                repository.as_ref(),
                selector.component(),
                selector.candidate().clone(),
            )
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
            let cargo_lock = manifests_at_candidate
                .cargo()
                .map(|surface| surface.lock().to_toml_text().as_str().to_string())
                .unwrap_or_default();
            let flake_lock = manifests_at_candidate
                .flake()
                .and_then(|surface| surface.lock().to_json_text().ok())
                .unwrap_or_default();
            locks.push(ComponentLockIdentity::from_text(
                selector.component().clone(),
                &cargo_lock,
                &flake_lock,
            ));
            manifests.push(manifests_at_candidate);
            let reference = PinnedFlakeReference::new(
                self.config.forge().owner().as_str().to_string(),
                selector.component().clone(),
                selector.candidate().clone(),
            );
            let prefetched = self
                .boundaries
                .nar_hash_source
                .prefetch(&reference)
                .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
            attestations.push(
                NixSourceAttestation::new(
                    selector.component().clone(),
                    selector.candidate().clone(),
                    prefetched.nar_hash().clone(),
                )
                .with_last_modified(prefetched.last_modified()),
            );
        }
        // Run the ordinary configured discovery as well as the release-train
        // owned-identity scan.  The latter deliberately retains an owned edge
        // even when its producer was omitted from configuration.
        DependencyGraph::discover(&self.config, &manifests)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        let admitted_externals = self.intent.immutable_externals().iter().fold(
            BTreeMap::new(),
            |mut admitted, external| {
                admitted
                    .entry(external.component().clone())
                    .or_insert_with(Vec::new)
                    .push(external.commit().clone());
                admitted
            },
        );
        let discovered = DependencyGraph::release_train_topology(
            self.config.forge().owner().as_str(),
            &admitted_externals,
            &manifests,
        )
        .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        ReleaseTrainResolution::new(
            self.intent.clone(),
            selectors.to_vec(),
            attestations,
            locks,
            discovered.internal().clone(),
            discovered.externals().clone(),
        )
        .resolve()
    }

    fn materialize_candidate(
        &self,
        component: &TrainComponent,
        selected: &CommitIdentifier,
        candidate_branch: &BranchName,
    ) -> Result<CommitIdentifier, ReleaseTrainError> {
        let repository = self.open_component(component.component())?;
        let message = CommitMessage::new(format!(
            "synchronizer: materialize release train {}",
            self.intent.name().as_str()
        ));
        let candidate = repository
            .commit_file_edits(selected, &[], &message)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        repository
            .push_train_branch(candidate_branch, &candidate)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        Ok(candidate)
    }

    fn open_component(
        &self,
        component: &ComponentName,
    ) -> Result<Box<dyn ComponentRepository>, ReleaseTrainError> {
        let clone_path = self
            .config
            .checkout_path(component)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        let remote_url = self
            .config
            .repository_url(component)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))?;
        self.boundaries
            .repository_opener
            .open(component, clone_path, remote_url)
            .map_err(|error| ReleaseTrainError::Infrastructure(error.to_string()))
    }
}

/// Candidate commits plus the ordinary cascade report. The caller records
/// real narHash and component lock evidence before emitting the immutable
/// closure; no universal lock is synthesized.
pub struct MaterializedReleaseTrain {
    intent: ReleaseTrainIntent,
    candidate_branch: BranchName,
    selectors: Vec<ResolvedSelector>,
    report: crate::report::SynchronizerReport,
    closure: ResolvedReleaseTrain,
}

impl MaterializedReleaseTrain {
    pub fn candidate_branch(&self) -> &BranchName {
        &self.candidate_branch
    }

    pub fn selectors(&self) -> &[ResolvedSelector] {
        &self.selectors
    }

    pub fn report(&self) -> &crate::report::SynchronizerReport {
        &self.report
    }

    /// The validated closure generated by the normal release-train run.
    pub fn closure(&self) -> &ResolvedReleaseTrain {
        &self.closure
    }

    /// Persist the generated JSON lock and integration flake chosen by the CLI.
    pub fn write_integration_artifacts(
        &self,
        output_directory: &Path,
        owner: &str,
    ) -> Result<ReleaseTrainArtifacts, ReleaseTrainError> {
        self.closure
            .write_integration_artifacts(output_directory, owner)
    }

    pub fn resolve_closure(
        &self,
        attestations: Vec<NixSourceAttestation>,
        locks: Vec<ComponentLockIdentity>,
        discovered_internal_components: BTreeSet<ComponentName>,
        discovered_external_components: BTreeMap<ComponentName, CommitIdentifier>,
    ) -> Result<ResolvedReleaseTrain, ReleaseTrainError> {
        ReleaseTrainResolution::new(
            self.intent.clone(),
            self.selectors.clone(),
            attestations,
            locks,
            discovered_internal_components,
            discovered_external_components,
        )
        .resolve()
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
    MissingSelectedBranch {
        component: ComponentName,
        branch: BranchName,
    },
    Infrastructure(String),
    Io(String),
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
            Self::MissingSelectedBranch { component, branch } => write!(
                formatter,
                "selected branch {} is absent for {}",
                branch.as_str(),
                component.as_str()
            ),
            Self::Infrastructure(detail) => {
                write!(formatter, "release-train infrastructure: {detail}")
            }
            Self::Io(detail) => write!(formatter, "release-train artifact IO: {detail}"),
        }
    }
}

impl std::error::Error for ReleaseTrainError {}
