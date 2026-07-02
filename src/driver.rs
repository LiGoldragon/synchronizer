//! The run driver: self-driving topological ascent from the leaves.
//!
//! The driver owns one run. It composes the boundaries (git, prefetch,
//! role directory, verifier, transitive-lock resolver) and walks the
//! algorithm of ARCHITECTURE.md §11: discover topology at pushed truth,
//! compute staleness against resolved targets, bump/commit/push/verify
//! level by level, collect every failure, and render one NOTA report at
//! the end.
//!
//! Only configuration load and topology discovery are run-fatal; every
//! per-repository failure is collected and the ascent continues.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::build_verify::{BuildVerifier, VerificationOutcome, Verifier, WireCheckClassifier};
use crate::cargo_manifest::{DependencyName, GitReference, PackageVersion};
use crate::component_manifests::ComponentManifests;
use crate::configuration::SynchronizerConfig;
use crate::error::Error;
use crate::flake_lock::{NarHashSource, NixFlakePrefetch, PinnedFlakeReference, PrefetchedSource};
use crate::git_repository::{
    CommitMessage, ComponentRepository, FileEdit, GitRepository, RepositoryFilePath,
};
use crate::report::{
    Action, AppliedBump, BumpRecord, Failure, FailureDetail, FailureStage, LevelOutcome, PinValue,
    PushedBranch, RepositoryOutcome, SynchronizerReport, Verification,
};
use crate::role_resolution::{ClusterRoleDirectory, CriomosClusterDirectory};
use crate::topology::{DependencyGraph, LocalPinName, PinLayer};
use crate::transitive_lock::{CargoUpdatePrecise, TransitiveLockRequest, TransitiveLockResolver};
use crate::types::{
    BranchName, BuilderRole, CommitIdentifier, ComponentName, RepositoryUrl, Timestamp, TomlText,
};
use crate::version_resolver::{ResolvedTarget, StalePin, VersionResolver};

/// Opens one component's git surface. [`GitRepositoryOpener`] is the
/// production implementation; fixtures stand in during ascent tests.
pub trait RepositoryOpener {
    fn open(
        &self,
        component: &ComponentName,
        clone_path: PathBuf,
        remote_url: RepositoryUrl,
    ) -> Result<Box<dyn ComponentRepository>, Error>;
}

/// The production opener: in-process gix over the configured clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepositoryOpener {
    /// The git binary used for the transport plumbing (ls-remote, fetch,
    /// push), normally `git` from PATH.
    git_binary: String,
}

impl GitRepositoryOpener {
    pub fn from_path_environment() -> Self {
        Self {
            git_binary: "git".to_string(),
        }
    }
}

impl RepositoryOpener for GitRepositoryOpener {
    fn open(
        &self,
        component: &ComponentName,
        clone_path: PathBuf,
        remote_url: RepositoryUrl,
    ) -> Result<Box<dyn ComponentRepository>, Error> {
        Ok(Box::new(GitRepository::open(
            component.clone(),
            clone_path,
            remote_url,
        )?))
    }
}

/// Binds a verifier to the role-resolved builder host, once per run.
pub trait VerifierSource {
    fn bind(
        &self,
        directory: &dyn ClusterRoleDirectory,
        role: &BuilderRole,
    ) -> Result<Box<dyn Verifier>, Error>;
}

/// The production source: [`BuildVerifier`] with the workspace's
/// wire-exercising check classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildVerifierSource {
    classifier: WireCheckClassifier,
}

impl BuildVerifierSource {
    pub fn workspace() -> Self {
        Self {
            classifier: WireCheckClassifier::workspace(),
        }
    }
}

impl VerifierSource for BuildVerifierSource {
    fn bind(
        &self,
        directory: &dyn ClusterRoleDirectory,
        role: &BuilderRole,
    ) -> Result<Box<dyn Verifier>, Error> {
        let _ = &self.classifier; // the workspace classifier is the built-in default
        Ok(Box::new(BuildVerifier::from_role(directory, role)?))
    }
}

/// The injected boundaries of one run.
pub struct RunBoundaries {
    pub repository_opener: Box<dyn RepositoryOpener>,
    pub nar_hash_source: Box<dyn NarHashSource>,
    pub role_directory: Box<dyn ClusterRoleDirectory>,
    pub verifier_source: Box<dyn VerifierSource>,
    pub lock_resolver: Box<dyn TransitiveLockResolver>,
}

/// One synchronizer run.
pub struct SynchronizerRun {
    config: SynchronizerConfig,
    boundaries: RunBoundaries,
}

/// One component loaded at run start: its git surface, remote `main` tip,
/// and manifests read at that tip.
struct LoadedComponent {
    repository: Box<dyn ComponentRepository>,
    manifests: ComponentManifests,
}

/// What one component's bump pass produced, before verification.
enum BumpOutcome {
    AlreadyAligned,
    Bumped {
        applied: Vec<AppliedBump>,
        tip: CommitIdentifier,
    },
    Failed(FailureStage),
}

impl SynchronizerRun {
    /// Compose a run from configuration and the production boundaries.
    pub fn new(config: SynchronizerConfig) -> Self {
        let role_directory = Box::new(CriomosClusterDirectory::new(
            config.cluster_configuration().clone(),
        ));
        Self::with_boundaries(
            config,
            RunBoundaries {
                repository_opener: Box::new(GitRepositoryOpener::from_path_environment()),
                nar_hash_source: Box::new(NixFlakePrefetch::from_path_environment()),
                role_directory,
                verifier_source: Box::new(BuildVerifierSource::workspace()),
                lock_resolver: Box::new(CargoUpdatePrecise::from_path_environment()),
            },
        )
    }

    /// Compose a run with explicit boundaries (fixture surface for ascent
    /// witnesses).
    pub fn with_boundaries(config: SynchronizerConfig, boundaries: RunBoundaries) -> Self {
        Self { config, boundaries }
    }

    /// The component name run-scoped failures (role resolution) are
    /// recorded under.
    fn run_scope_component() -> ComponentName {
        ComponentName::new("synchronizer")
    }

    /// Execute the ascent and return the collected report.
    ///
    /// `Err` only for run-fatal conditions (unreadable configuration,
    /// undiscoverable topology, dependency cycle). Everything else —
    /// including every failed bump, push, and verify — lands inside the
    /// report.
    pub fn execute(self) -> Result<SynchronizerReport, Error> {
        let started_at = Timestamp::now();
        let mut failures: Vec<Failure> = Vec::new();
        let mut unprocessable: Vec<ComponentName> = Vec::new();
        let mut loaded: BTreeMap<ComponentName, LoadedComponent> = BTreeMap::new();

        // 1–3: open each clone, query the remote main tip, fetch it, and
        // read the manifests at it. A component that fails here is
        // reported and excluded from the ascent; the run keeps going.
        for component in self.config.components() {
            let name = component.name().clone();
            match self.load_component(&name) {
                Ok(loaded_component) => {
                    loaded.insert(name, loaded_component);
                }
                Err(error) => {
                    failures.push(Failure::new(
                        name.clone(),
                        FailureStage::Fetch,
                        FailureDetail::new(error.to_string()),
                    ));
                    unprocessable.push(name);
                }
            }
        }

        // 4: discover the graph and the ascent order — the only run-fatal
        // stage after configuration load.
        let manifest_list: Vec<ComponentManifests> = loaded
            .values()
            .map(|component| component.manifests.clone())
            .collect();
        let graph = DependencyGraph::discover(&self.config, &manifest_list)?;
        let levels = graph.ascent_levels()?;

        let main_tips: BTreeMap<ComponentName, CommitIdentifier> = loaded
            .iter()
            .map(|(name, component)| (name.clone(), component.manifests.base_revision().clone()))
            .collect();
        let mut resolver = VersionResolver::new(main_tips);

        // Role resolution happens once; its failure never stops bumps and
        // pushes — every verification then reports NotAttempted (§9).
        let verifier: Option<Box<dyn Verifier>> = match self.boundaries.verifier_source.bind(
            self.boundaries.role_directory.as_ref(),
            self.config.builder_role(),
        ) {
            Ok(verifier) => Some(verifier),
            Err(error) => {
                failures.push(Failure::new(
                    Self::run_scope_component(),
                    FailureStage::RoleResolution,
                    FailureDetail::new(error.to_string()),
                ));
                None
            }
        };

        let mut prefetch_cache: BTreeMap<(ComponentName, String), PrefetchedSource> =
            BTreeMap::new();

        // 5: the ascent.
        let mut level_outcomes: Vec<LevelOutcome> = Vec::new();
        for (index, level) in levels.levels().iter().enumerate() {
            let mut outcomes = Vec::new();
            for component in level {
                let outcome = self.process_component(
                    component,
                    &graph,
                    &mut resolver,
                    &loaded,
                    verifier.as_deref(),
                    &mut failures,
                    &mut prefetch_cache,
                );
                outcomes.push(outcome);
            }
            level_outcomes.push(LevelOutcome::new(index as u32, outcomes));
        }

        // Components that could not join the ascent close the report in a
        // trailing level of their own.
        if !unprocessable.is_empty() {
            let outcomes = unprocessable
                .into_iter()
                .map(|name| {
                    RepositoryOutcome::new(
                        name,
                        Action::BumpFailed(FailureStage::Fetch),
                        Verification::NotAttempted,
                    )
                })
                .collect();
            level_outcomes.push(LevelOutcome::new(levels.levels().len() as u32, outcomes));
        }

        Ok(SynchronizerReport::new(
            started_at,
            Timestamp::now(),
            level_outcomes,
            failures,
        ))
    }

    fn load_component(&self, name: &ComponentName) -> Result<LoadedComponent, Error> {
        let clone_path = self.config.checkout_path(name)?;
        let remote_url = self.config.repository_url(name)?;
        let repository = self
            .boundaries
            .repository_opener
            .open(name, clone_path, remote_url)?;
        let tip = repository.remote_main_tip()?;
        repository.fetch(&tip)?;
        let manifests = ComponentManifests::load_at(repository.as_ref(), name, tip)?;
        Ok(LoadedComponent {
            repository,
            manifests,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn process_component(
        &self,
        component: &ComponentName,
        graph: &DependencyGraph,
        resolver: &mut VersionResolver,
        loaded: &BTreeMap<ComponentName, LoadedComponent>,
        verifier: Option<&dyn Verifier>,
        failures: &mut Vec<Failure>,
        prefetch_cache: &mut BTreeMap<(ComponentName, String), PrefetchedSource>,
    ) -> RepositoryOutcome {
        let mut collect = |stage: FailureStage, detail: String| {
            failures.push(Failure::new(
                component.clone(),
                stage,
                FailureDetail::new(detail),
            ));
        };
        let Some(loaded_component) = loaded.get(component) else {
            return RepositoryOutcome::new(
                component.clone(),
                Action::BumpFailed(FailureStage::Fetch),
                Verification::NotAttempted,
            );
        };
        let edges = graph.dependencies_of(component);
        let stale = match resolver.stale_pins(&loaded_component.manifests, &edges) {
            Ok(stale) => stale,
            Err(error) => {
                collect(FailureStage::Resolve, error.to_string());
                return RepositoryOutcome::new(
                    component.clone(),
                    Action::BumpFailed(FailureStage::Resolve),
                    Verification::NotAttempted,
                );
            }
        };
        if stale.is_empty() {
            return RepositoryOutcome::new(
                component.clone(),
                Action::AlreadyAligned,
                Verification::NotAttempted,
            );
        }
        let bump = self.apply_bumps(
            component,
            loaded_component,
            &stale,
            loaded,
            failures,
            prefetch_cache,
        );
        match bump {
            BumpOutcome::AlreadyAligned => RepositoryOutcome::new(
                component.clone(),
                Action::AlreadyAligned,
                Verification::NotAttempted,
            ),
            BumpOutcome::Failed(stage) => RepositoryOutcome::new(
                component.clone(),
                Action::BumpFailed(stage),
                Verification::NotAttempted,
            ),
            BumpOutcome::Bumped { applied, tip } => {
                resolver.record_bump(component.clone(), tip.clone());
                let verification = match verifier {
                    None => Verification::NotAttempted,
                    Some(verifier) => match verifier.verify(self.config.forge(), component, &tip) {
                        VerificationOutcome::Verified(gate) => {
                            Verification::Verified(verifier.host().clone(), gate)
                        }
                        VerificationOutcome::Failed(failure) => {
                            failures.push(Failure::new(
                                component.clone(),
                                FailureStage::Verify,
                                FailureDetail::new(failure.detail().to_string()),
                            ));
                            Verification::VerifyFailed(verifier.host().clone())
                        }
                    },
                };
                RepositoryOutcome::new(
                    component.clone(),
                    Action::Bumped(BumpRecord::new(
                        applied,
                        PushedBranch::new(BranchName::synchronizer(), tip),
                    )),
                    verification,
                )
            }
        }
    }

    /// Apply every stale pin as a typed edit on clones of the consumer's
    /// surfaces, commit the edited files on the remote main tip, and push
    /// the synchronizer branch.
    fn apply_bumps(
        &self,
        component: &ComponentName,
        loaded_component: &LoadedComponent,
        stale: &[StalePin],
        loaded: &BTreeMap<ComponentName, LoadedComponent>,
        failures: &mut Vec<Failure>,
        prefetch_cache: &mut BTreeMap<(ComponentName, String), PrefetchedSource>,
    ) -> BumpOutcome {
        let mut work = loaded_component.manifests.clone();
        let mut applied: Vec<AppliedBump> = Vec::new();
        let mut touched: Vec<PinLayer> = Vec::new();
        let mut lock_gap: Option<(DependencyName, CommitIdentifier)> = None;

        for pin in stale {
            let producer = pin.edge().producer().clone();
            let target = pin.target();
            let stage_result: Result<AppliedBump, (FailureStage, Error)> =
                match (pin.edge().layer(), pin.edge().local_name()) {
                    (PinLayer::CargoLock, LocalPinName::CargoPackage(name)) => self
                        .bump_cargo_lock(component, name, &producer, target, &mut work, loaded)
                        .map_err(|error| (FailureStage::LockEdit, error)),
                    (PinLayer::CargoManifest, LocalPinName::CargoPackage(name)) => self
                        .bump_cargo_manifest(name, &producer, target, &mut work)
                        .map_err(|error| (FailureStage::ManifestEdit, error)),
                    (PinLayer::FlakeLock, LocalPinName::FlakeInput(input)) => self
                        .bump_flake_lock(
                            component,
                            input,
                            &producer,
                            target,
                            &mut work,
                            prefetch_cache,
                        )
                        .map_err(|error| {
                            let stage = match &error {
                                Error::NarHashPrefetch { .. } => FailureStage::Prefetch,
                                _ => FailureStage::LockEdit,
                            };
                            (stage, error)
                        }),
                    (PinLayer::FlakeManifest, LocalPinName::FlakeInput(input)) => self
                        .bump_flake_manifest(component, input, &producer, target, pin, &mut work)
                        .map_err(|error| (FailureStage::ManifestEdit, error)),
                    (layer, name) => Err((
                        FailureStage::Resolve,
                        Error::ManifestDecode {
                            component: component.clone(),
                            layer,
                            detail: format!("edge layer {layer:?} does not match {name:?}"),
                        },
                    )),
                };
            match stage_result {
                Ok(bump) => {
                    touched.push(pin.edge().layer());
                    applied.push(bump);
                    if pin.edge().layer() == PinLayer::CargoLock
                        && let LocalPinName::CargoPackage(name) = pin.edge().local_name()
                        && self.cargo_lock_has_gap(name, &producer, &work, loaded)
                    {
                        lock_gap = Some((name.clone(), target.revision().clone()));
                    }
                }
                Err((stage, error)) => {
                    failures.push(Failure::new(
                        component.clone(),
                        stage,
                        FailureDetail::new(error.to_string()),
                    ));
                    return BumpOutcome::Failed(stage);
                }
            }
        }

        // The controlled transitive fallback where the typed lock edit left
        // a gap. Its own failure keeps the typed lock: build-verify owns
        // the final word.
        let mut lock_text_override: Option<TomlText> = None;
        if let Some((package, revision)) = lock_gap
            && let Some(cargo) = work.cargo()
        {
            let request_result = loaded_component
                .repository
                .tree_files_at(loaded_component.manifests.base_revision())
                .map(|base_tree| TransitiveLockRequest {
                    consumer: component.clone(),
                    base_tree,
                    edited_manifest: cargo.manifest().to_toml_text(),
                    edited_lock: cargo.lock().to_toml_text(),
                    package,
                    revision,
                });
            match request_result {
                Ok(request) => match self.boundaries.lock_resolver.resolve_lock(&request) {
                    Ok(refreshed) => {
                        lock_text_override = Some(refreshed);
                    }
                    Err(error) => {
                        failures.push(Failure::new(
                            component.clone(),
                            FailureStage::LockEdit,
                            FailureDetail::new(error.to_string()),
                        ));
                    }
                },
                Err(error) => {
                    failures.push(Failure::new(
                        component.clone(),
                        FailureStage::LockEdit,
                        FailureDetail::new(error.to_string()),
                    ));
                }
            }
        }

        if applied.is_empty() {
            return BumpOutcome::AlreadyAligned;
        }

        // Render exactly the touched files.
        let mut edits: Vec<FileEdit> = Vec::new();
        if touched.contains(&PinLayer::CargoManifest)
            && let Some(cargo) = work.cargo()
        {
            edits.push(FileEdit::new(
                RepositoryFilePath::cargo_manifest(),
                cargo.manifest().to_toml_text().as_str().to_string(),
            ));
        }
        if touched.contains(&PinLayer::CargoLock)
            && let Some(cargo) = work.cargo()
        {
            let text = lock_text_override.unwrap_or_else(|| cargo.lock().to_toml_text());
            edits.push(FileEdit::new(
                RepositoryFilePath::cargo_lock(),
                text.as_str().to_string(),
            ));
        }
        if touched.contains(&PinLayer::FlakeManifest)
            && let Some(flake) = work.flake()
        {
            edits.push(FileEdit::new(
                RepositoryFilePath::flake_manifest(),
                flake.manifest().to_nix_text(),
            ));
        }
        if touched.contains(&PinLayer::FlakeLock)
            && let Some(flake) = work.flake()
        {
            match flake.lock().to_json_text() {
                Ok(text) => edits.push(FileEdit::new(RepositoryFilePath::flake_lock(), text)),
                Err(error) => {
                    failures.push(Failure::new(
                        component.clone(),
                        FailureStage::LockEdit,
                        FailureDetail::new(error.to_string()),
                    ));
                    return BumpOutcome::Failed(FailureStage::LockEdit);
                }
            }
        }

        let message = CommitMessage::new(Self::render_commit_message(&applied));
        let base = loaded_component.manifests.base_revision();
        let tip = match loaded_component
            .repository
            .commit_file_edits(base, &edits, &message)
        {
            Ok(tip) => tip,
            Err(error) => {
                failures.push(Failure::new(
                    component.clone(),
                    FailureStage::Commit,
                    FailureDetail::new(error.to_string()),
                ));
                return BumpOutcome::Failed(FailureStage::Commit);
            }
        };
        if let Err(error) = loaded_component.repository.push_synchronizer_branch(&tip) {
            failures.push(Failure::new(
                component.clone(),
                FailureStage::Push,
                FailureDetail::new(error.to_string()),
            ));
            return BumpOutcome::Failed(FailureStage::Push);
        }
        BumpOutcome::Bumped { applied, tip }
    }

    fn bump_cargo_lock(
        &self,
        component: &ComponentName,
        name: &DependencyName,
        producer: &ComponentName,
        target: &ResolvedTarget,
        work: &mut ComponentManifests,
        loaded: &BTreeMap<ComponentName, LoadedComponent>,
    ) -> Result<AppliedBump, Error> {
        let version_at_target = self.producer_version(name, producer, loaded);
        let cargo = work.cargo_mut().ok_or_else(|| Error::ManifestDecode {
            component: component.clone(),
            layer: PinLayer::CargoLock,
            detail: "cargo surface absent".to_string(),
        })?;
        let existing_reference = cargo
            .lock()
            .git_packages()?
            .into_iter()
            .find(|(package, _)| package == name)
            .map(|(_, pin)| pin.reference().clone())
            .unwrap_or(GitReference::DefaultBranch);
        let reference = match target {
            ResolvedTarget::RemoteMainTip(_) => existing_reference,
            ResolvedTarget::SynchronizerTip(_) => GitReference::Branch(BranchName::synchronizer()),
        };
        let previous = cargo.lock_mut().repin_git_package(
            name,
            reference,
            target.revision().clone(),
            version_at_target,
        )?;
        Ok(AppliedBump::new(
            producer.clone(),
            PinLayer::CargoLock,
            PinValue::Revision(previous),
            PinValue::Revision(target.revision().clone()),
        ))
    }

    fn bump_cargo_manifest(
        &self,
        name: &DependencyName,
        producer: &ComponentName,
        target: &ResolvedTarget,
        work: &mut ComponentManifests,
    ) -> Result<AppliedBump, Error> {
        let cargo = work.cargo_mut().ok_or_else(|| Error::ManifestDecode {
            component: producer.clone(),
            layer: PinLayer::CargoManifest,
            detail: "cargo surface absent".to_string(),
        })?;
        let next_branch = target.reachable_branch();
        let previous = cargo
            .manifest_mut()
            .redirect_git_dependency(name, GitReference::Branch(next_branch.clone()))?;
        let previous_value = match previous {
            GitReference::Branch(branch) => PinValue::Reference(branch),
            GitReference::Tag(tag) => PinValue::Reference(BranchName::new(tag)),
            GitReference::Revision(revision) => PinValue::Revision(revision),
            GitReference::DefaultBranch => PinValue::Reference(BranchName::main()),
        };
        Ok(AppliedBump::new(
            producer.clone(),
            PinLayer::CargoManifest,
            previous_value,
            PinValue::Reference(next_branch),
        ))
    }

    fn bump_flake_lock(
        &self,
        component: &ComponentName,
        input: &crate::flake_lock::InputName,
        producer: &ComponentName,
        target: &ResolvedTarget,
        work: &mut ComponentManifests,
        prefetch_cache: &mut BTreeMap<(ComponentName, String), PrefetchedSource>,
    ) -> Result<AppliedBump, Error> {
        let owner = self.config.forge().owner().as_str().to_string();
        let cache_key = (producer.clone(), target.revision().as_str().to_string());
        let prefetched = match prefetch_cache.get(&cache_key) {
            Some(prefetched) => prefetched.clone(),
            None => {
                let reference =
                    PinnedFlakeReference::new(owner, producer.clone(), target.revision().clone());
                let prefetched = self.boundaries.nar_hash_source.prefetch(&reference)?;
                prefetch_cache.insert(cache_key, prefetched.clone());
                prefetched
            }
        };
        // The node's `original` is always preserved: the locked rev alone
        // carries the cascade. Nix re-resolves originals from flake.nix on
        // update, so an original edited to follow the synchronizer branch
        // would be discarded and the input re-locked to main.
        let flake = work.flake_mut().ok_or_else(|| Error::ManifestDecode {
            component: component.clone(),
            layer: PinLayer::FlakeLock,
            detail: "flake surface absent".to_string(),
        })?;
        let previous = flake.lock_mut().repin_input(
            component,
            input,
            target.revision().clone(),
            prefetched,
        )?;
        Ok(AppliedBump::new(
            producer.clone(),
            PinLayer::FlakeLock,
            PinValue::Revision(previous),
            PinValue::Revision(target.revision().clone()),
        ))
    }

    fn bump_flake_manifest(
        &self,
        component: &ComponentName,
        input: &crate::flake_lock::InputName,
        producer: &ComponentName,
        target: &ResolvedTarget,
        pin: &StalePin,
        work: &mut ComponentManifests,
    ) -> Result<AppliedBump, Error> {
        let flake = work.flake_mut().ok_or_else(|| Error::ManifestDecode {
            component: component.clone(),
            layer: PinLayer::FlakeManifest,
            detail: "flake surface absent".to_string(),
        })?;
        flake.manifest_mut().rewrite_pinned_input(
            component,
            input,
            GitReference::Revision(target.revision().clone()),
        )?;
        Ok(AppliedBump::new(
            producer.clone(),
            PinLayer::FlakeManifest,
            pin.pinned().clone(),
            PinValue::Revision(target.revision().clone()),
        ))
    }

    /// The version the producer's own manifest declares at the target
    /// revision — available when the producer's root manifest publishes the
    /// pinned package. `None` (a workspace-member package) routes version
    /// truth to the fallback and the verify.
    fn producer_version(
        &self,
        name: &DependencyName,
        producer: &ComponentName,
        loaded: &BTreeMap<ComponentName, LoadedComponent>,
    ) -> Option<PackageVersion> {
        let manifests = &loaded.get(producer)?.manifests;
        let manifest = manifests.cargo()?.manifest();
        if manifest.package_name()? == name {
            manifest.package_version().cloned()
        } else {
            None
        }
    }

    /// Whether the typed repin left a transitive gap: the producer's
    /// manifest at the target revision declares a dependency the consumer's
    /// lock does not record for it, or the producer's published version is
    /// unknowable from its root manifest.
    fn cargo_lock_has_gap(
        &self,
        name: &DependencyName,
        producer: &ComponentName,
        work: &ComponentManifests,
        loaded: &BTreeMap<ComponentName, LoadedComponent>,
    ) -> bool {
        let Some(cargo) = work.cargo() else {
            return false;
        };
        let Some(recorded) = cargo.lock().recorded_dependencies_of(name) else {
            return true;
        };
        let Some(producer_loaded) = loaded.get(producer) else {
            return false;
        };
        let Some(producer_cargo) = producer_loaded.manifests.cargo() else {
            return false;
        };
        if producer_cargo.manifest().package_name() != Some(name) {
            // Workspace-member package: the root manifest cannot answer for
            // its dependency set; let cargo complete the graph.
            return true;
        }
        producer_cargo
            .manifest()
            .declared_dependency_package_names()
            .iter()
            .any(|declared| !recorded.contains(declared))
    }

    fn render_commit_message(applied: &[AppliedBump]) -> String {
        let mut lines = vec![
            "synchronizer: cascade dependency bumps".to_string(),
            String::new(),
        ];
        for bump in applied {
            let next = match bump.next() {
                PinValue::Revision(revision) => revision.as_str().to_string(),
                PinValue::Reference(branch) => branch.as_str().to_string(),
            };
            lines.push(format!(
                "- {} ({:?}) -> {}",
                bump.dependency().as_str(),
                bump.layer(),
                next
            ));
        }
        lines.join("\n")
    }
}
