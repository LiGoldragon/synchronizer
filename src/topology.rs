//! Dependency topology, discovered — never declared.
//!
//! The configuration names the participating components; the edges between
//! them come only from the manifests: Cargo git dependencies and lock pins
//! matched by repository URL, flake URL-pinned inputs and lock inputs
//! matched by owner and repository. The result must be a DAG; a cycle is
//! run-fatal because it admits no topological ascent.

use std::collections::{BTreeMap, BTreeSet};

use nota::{NotaDecode, NotaEncode};

use crate::cargo_manifest::DependencyName;
use crate::component_manifests::ComponentManifests;
use crate::configuration::SynchronizerConfig;
use crate::error::Error;
use crate::flake_lock::InputName;
use crate::types::{CommitIdentifier, ComponentName};

/// Which pin surface an edge was discovered in (and therefore where its
/// bump must be written).
#[derive(Debug, Clone, Copy, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum PinLayer {
    CargoManifest,
    CargoLock,
    FlakeManifest,
    FlakeLock,
}

/// The name a consumer pins a producer under in its own manifests: the
/// Cargo package name for Cargo layers, the flake input name for flake
/// layers. Component identity never goes through this name — it exists so
/// a bump can address the entry it must edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalPinName {
    CargoPackage(DependencyName),
    FlakeInput(InputName),
}

/// One consumer-to-producer pin discovered in the consumer's manifests.
///
/// A consumer usually holds several edges to the same producer — one per
/// layer that pins it (`CargoLock` and `FlakeLock` for a typical sibling
/// dependency). Each layer bumps independently and is reported
/// independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEdge {
    consumer: ComponentName,
    producer: ComponentName,
    layer: PinLayer,
    local_name: LocalPinName,
}

impl DependencyEdge {
    pub fn new(
        consumer: ComponentName,
        producer: ComponentName,
        layer: PinLayer,
        local_name: LocalPinName,
    ) -> Self {
        Self {
            consumer,
            producer,
            layer,
            local_name,
        }
    }

    pub fn consumer(&self) -> &ComponentName {
        &self.consumer
    }

    pub fn producer(&self) -> &ComponentName {
        &self.producer
    }

    pub fn layer(&self) -> PinLayer {
        self.layer
    }

    pub fn local_name(&self) -> &LocalPinName {
        &self.local_name
    }
}

/// The dependency DAG over the configured components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraph {
    components: Vec<ComponentName>,
    edges: Vec<DependencyEdge>,
}

impl DependencyGraph {
    /// Discover every edge from the loaded manifests. A git dependency or
    /// flake input pointing outside the configured component set is not an
    /// edge (third-party inputs are out of scope by design).
    pub fn discover(
        configuration: &SynchronizerConfig,
        manifests: &[ComponentManifests],
    ) -> Result<Self, Error> {
        let configured: BTreeSet<&ComponentName> = configuration
            .components()
            .iter()
            .map(|component| component.name())
            .collect();
        let owner = configuration.forge().owner();
        let mut edges = Vec::new();
        for consumer in manifests {
            let consumer_name = consumer.component().clone();
            if let Some(cargo) = consumer.cargo() {
                for (dependency_name, source) in cargo.manifest().git_dependencies() {
                    let Ok(identity) = source.url().repository_identity() else {
                        continue;
                    };
                    if identity.owner == owner.as_str() && configured.contains(&identity.repository)
                    {
                        edges.push(DependencyEdge::new(
                            consumer_name.clone(),
                            identity.repository.clone(),
                            PinLayer::CargoManifest,
                            LocalPinName::CargoPackage(dependency_name),
                        ));
                    }
                }
                for (package_name, pin) in cargo.lock().git_packages()? {
                    let Ok(identity) = pin.url().repository_identity() else {
                        continue;
                    };
                    if identity.owner == owner.as_str() && configured.contains(&identity.repository)
                    {
                        edges.push(DependencyEdge::new(
                            consumer_name.clone(),
                            identity.repository.clone(),
                            PinLayer::CargoLock,
                            LocalPinName::CargoPackage(package_name),
                        ));
                    }
                }
            }
            if let Some(flake) = consumer.flake() {
                for occurrence in flake.manifest().pinned_inputs() {
                    let Some((url_owner, repository)) = occurrence.url().github_identity() else {
                        continue;
                    };
                    if url_owner == owner.as_str() && configured.contains(repository) {
                        edges.push(DependencyEdge::new(
                            consumer_name.clone(),
                            repository.clone(),
                            PinLayer::FlakeManifest,
                            LocalPinName::FlakeInput(occurrence.input().clone()),
                        ));
                    }
                }
                for (input_name, locked) in flake.lock().github_inputs() {
                    let Some(repository) = locked.component_name() else {
                        continue;
                    };
                    if locked.owner() == Some(owner.as_str()) && configured.contains(&repository) {
                        edges.push(DependencyEdge::new(
                            consumer_name.clone(),
                            repository,
                            PinLayer::FlakeLock,
                            LocalPinName::FlakeInput(input_name),
                        ));
                    }
                }
            }
        }
        let components = manifests
            .iter()
            .map(|manifest| manifest.component().clone())
            .collect();
        // A producer declared under several same-name manifest entries (the
        // same crate in `[dependencies]` and `[dev-dependencies]`) yields one
        // discovered edge per entry above; collapse them so the invariant
        // "one edge per (consumer, producer, layer, local name)" holds. The
        // layer's bump and report then run once, and the manifest editor
        // redirects every textual entry behind that single edge.
        let edges = Self::deduplicated(edges);
        Ok(Self { components, edges })
    }

    /// Remove duplicate edges while preserving first-occurrence order — the
    /// order that drives the deterministic bump-and-report sequence.
    fn deduplicated(edges: Vec<DependencyEdge>) -> Vec<DependencyEdge> {
        let mut unique: Vec<DependencyEdge> = Vec::new();
        for edge in edges {
            if !unique.contains(&edge) {
                unique.push(edge);
            }
        }
        unique
    }

    pub fn edges(&self) -> &[DependencyEdge] {
        &self.edges
    }

    /// Classify every owned selected-manifest dependency for a release train.
    /// A source can leave the train only when its exact resolved lock commit is
    /// explicitly admitted. Manifest-only identities remain internal because
    /// they do not supply the immutable commit an external admission requires.
    pub fn release_train_topology(
        owner: &str,
        admitted_externals: &BTreeMap<ComponentName, Vec<CommitIdentifier>>,
        manifests: &[ComponentManifests],
    ) -> Result<ReleaseTrainTopology, Error> {
        let mut internal = manifests
            .iter()
            .map(|manifest| manifest.component().clone())
            .collect::<BTreeSet<_>>();
        let mut manifest_identities = BTreeSet::new();
        let mut locked_identities: BTreeMap<ComponentName, Vec<CommitIdentifier>> = BTreeMap::new();
        for manifest in manifests {
            if let Some(cargo) = manifest.cargo() {
                for (_, source) in cargo.manifest().git_dependencies() {
                    if let Ok(identity) = source.url().repository_identity()
                        && identity.owner == owner
                    {
                        manifest_identities.insert(identity.repository);
                    }
                }
                for (_, pin) in cargo.lock().git_packages()? {
                    if let Ok(identity) = pin.url().repository_identity()
                        && identity.owner == owner
                    {
                        let commits = locked_identities.entry(identity.repository).or_default();
                        if !commits.contains(pin.revision()) {
                            commits.push(pin.revision().clone());
                        }
                    }
                }
            }
            if let Some(flake) = manifest.flake() {
                for occurrence in flake.manifest().pinned_inputs() {
                    if let Some((input_owner, repository)) = occurrence.url().github_identity()
                        && input_owner == owner
                    {
                        manifest_identities.insert(repository.clone());
                    }
                }
                for (_, locked) in flake.lock().github_inputs() {
                    if locked.owner() == Some(owner)
                        && let (Some(repository), Some(revision)) =
                            (locked.component_name(), locked.revision())
                    {
                        let commits = locked_identities.entry(repository).or_default();
                        if !commits.contains(&revision) {
                            commits.push(revision);
                        }
                    }
                }
            }
        }
        let mut externals = BTreeMap::new();
        let mut admission_mismatches = Vec::new();
        for (component, commits) in &locked_identities {
            let admitted = admitted_externals.get(component);
            if commits.len() == 1
                && let Some(commit) = commits.first()
                && admitted.is_some_and(|admitted| admitted.contains(commit))
            {
                externals.insert(component.clone(), commit.clone());
            } else {
                if let Some(admitted) = admitted {
                    admission_mismatches.push(ImmutableExternalAdmissionMismatch::new(
                        component.clone(),
                        commits.clone(),
                        admitted.clone(),
                    ));
                }
                internal.insert(component.clone());
            }
        }
        for component in manifest_identities {
            if !externals.contains_key(&component) {
                internal.insert(component);
            }
        }
        Ok(ReleaseTrainTopology {
            internal,
            externals,
            admission_mismatches,
        })
    }

    /// All edges whose consumer is `consumer`.
    pub fn dependencies_of(&self, consumer: &ComponentName) -> Vec<&DependencyEdge> {
        self.edges
            .iter()
            .filter(|edge| edge.consumer() == consumer)
            .collect()
    }

    /// The ascent order: level 0 holds the leaves (components with no
    /// component dependencies), level N holds components all of whose
    /// dependencies sit in levels below N. `Err(DependencyCycle)` when the
    /// graph is not a DAG — only genuine cycles among the graph's own
    /// members; an edge to a producer that failed to load is not a cycle.
    /// Kahn's algorithm; deterministic name order within a level.
    pub fn ascent_levels(&self) -> Result<TopologicalLevels, Error> {
        let members: BTreeSet<&ComponentName> = self.components.iter().collect();
        let mut remaining_dependencies: BTreeMap<&ComponentName, BTreeSet<&ComponentName>> = self
            .components
            .iter()
            .map(|component| (component, BTreeSet::new()))
            .collect();
        for edge in &self.edges {
            if edge.consumer() == edge.producer() {
                continue; // self-pins cannot order the ascent
            }
            if !members.contains(edge.producer()) {
                // A producer outside the graph's member set — configured
                // but not loaded this run (its fetch failed) — cannot
                // order the ascent and is not a cycle. Its consumers are
                // placed; resolving the edge then fails and is collected
                // (§9 collect-and-continue).
                continue;
            }
            if let Some(producers) = remaining_dependencies.get_mut(edge.consumer()) {
                producers.insert(edge.producer());
            }
        }
        let mut levels: Vec<Vec<ComponentName>> = Vec::new();
        let mut placed: BTreeSet<&ComponentName> = BTreeSet::new();
        while placed.len() < self.components.len() {
            let level: Vec<&ComponentName> = remaining_dependencies
                .iter()
                .filter(|(component, producers)| {
                    !placed.contains(*component)
                        && producers.iter().all(|producer| placed.contains(producer))
                })
                .map(|(component, _)| *component)
                .collect();
            if level.is_empty() {
                let members = remaining_dependencies
                    .keys()
                    .filter(|component| !placed.contains(*component))
                    .map(|component| (*component).clone())
                    .collect();
                return Err(Error::DependencyCycle { members });
            }
            for component in &level {
                placed.insert(component);
            }
            levels.push(level.into_iter().cloned().collect());
        }
        Ok(TopologicalLevels(levels))
    }
}

/// The strict owned-source classification consumed by release-train closure
/// validation. Internal sources must be declared train members; external
/// sources carry the exact admitted locked commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTrainTopology {
    internal: BTreeSet<ComponentName>,
    externals: BTreeMap<ComponentName, CommitIdentifier>,
    admission_mismatches: Vec<ImmutableExternalAdmissionMismatch>,
}

/// Typed evidence that an authored ImmutableExternal named a repository but
/// did not admit the exact lock commit observed in selected candidate truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmutableExternalAdmissionMismatch {
    component: ComponentName,
    observed: Vec<CommitIdentifier>,
    admitted: Vec<CommitIdentifier>,
}

impl ImmutableExternalAdmissionMismatch {
    pub fn new(
        component: ComponentName,
        observed: Vec<CommitIdentifier>,
        admitted: Vec<CommitIdentifier>,
    ) -> Self {
        Self {
            component,
            observed,
            admitted,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn observed(&self) -> &[CommitIdentifier] {
        &self.observed
    }

    pub fn admitted(&self) -> &[CommitIdentifier] {
        &self.admitted
    }
}

impl ReleaseTrainTopology {
    pub fn internal(&self) -> &BTreeSet<ComponentName> {
        &self.internal
    }

    pub fn externals(&self) -> &BTreeMap<ComponentName, CommitIdentifier> {
        &self.externals
    }

    pub fn admission_mismatches(&self) -> &[ImmutableExternalAdmissionMismatch] {
        &self.admission_mismatches
    }
}

/// Components grouped by topological level, leaves first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologicalLevels(Vec<Vec<ComponentName>>);

impl TopologicalLevels {
    pub fn levels(&self) -> &[Vec<ComponentName>] {
        &self.0
    }
}
