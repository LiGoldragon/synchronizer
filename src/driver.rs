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

use crate::build_verify::{BuildVerifier, VerificationOutcome, Verifier, VerifyPolicy};
use crate::cargo_manifest::{DependencyName, GitReference, PackageVersion};
use crate::component_manifests::ComponentManifests;
use crate::configuration::{BranchScheme, BuilderResolution, CommitAuthor, SynchronizerConfig};
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
    BranchName, BuilderHost, CommitIdentifier, ComponentName, RepositoryUrl, Timestamp, TomlText,
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

/// The production opener: in-process gix over the configured clone, carrying
/// the configured branch scheme and commit author so every opened repository
/// queries the right mainline, pushes the right staging branch, and stamps
/// the right author.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepositoryOpener {
    /// The git binary used for the transport plumbing (ls-remote, fetch,
    /// push), normally `git` from PATH.
    git_binary: String,
    branch_scheme: BranchScheme,
    commit_author: CommitAuthor,
}

impl GitRepositoryOpener {
    pub fn from_path_environment(branch_scheme: BranchScheme, commit_author: CommitAuthor) -> Self {
        Self {
            git_binary: "git".to_string(),
            branch_scheme,
            commit_author,
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
            self.branch_scheme.clone(),
            self.commit_author.clone(),
        )?))
    }
}

/// Resolves the build-verify host once per run, however configuration chose
/// to determine it (direct host or role through a cluster directory).
pub trait BuilderHostResolver {
    fn resolve(&self) -> Result<BuilderHost, Error>;
}

/// The production resolver: dispatches on the configured
/// [`BuilderResolution`] strategy. A direct host is returned as-is; a cluster
/// role is resolved through the CriomOS cluster directory. Neither path is
/// hard-coded — the strategy comes entirely from configuration.
pub struct ConfiguredBuilderHost {
    resolution: BuilderResolution,
}

impl ConfiguredBuilderHost {
    pub fn new(resolution: BuilderResolution) -> Self {
        Self { resolution }
    }
}

impl BuilderHostResolver for ConfiguredBuilderHost {
    fn resolve(&self) -> Result<BuilderHost, Error> {
        match &self.resolution {
            BuilderResolution::DirectHost(host) => Ok(host.clone()),
            BuilderResolution::ClusterRole(role, source) => {
                CriomosClusterDirectory::new(source.clone()).host_for(role)
            }
        }
    }
}

/// Binds a verifier to the resolved builder host, once per run.
pub trait VerifierSource {
    fn bind(&self, host: BuilderHost) -> Result<Box<dyn Verifier>, Error>;
}

/// The production source: [`BuildVerifier`] with the configured verify
/// policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildVerifierSource {
    policy: VerifyPolicy,
}

impl BuildVerifierSource {
    pub fn new(policy: VerifyPolicy) -> Self {
        Self { policy }
    }
}

impl VerifierSource for BuildVerifierSource {
    fn bind(&self, host: BuilderHost) -> Result<Box<dyn Verifier>, Error> {
        Ok(Box::new(BuildVerifier::new(host, self.policy.clone())))
    }
}

/// Which branch a run reads each component's base manifests from, and how the
/// cascade ledger starts.
///
/// A typed run mode, not a flag: the two variants name the two coordinated
/// flows. Neither carries a branch name — the staging branch is always the
/// configured `BranchScheme.staging`, and the pre-staged producer set is
/// discovered from the forge (a component whose staging branch exists), never
/// named in code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseSelection {
    /// Ordinary version propagation: read every component at its mainline tip;
    /// the cascade ledger starts empty. A producer bumped during the ascent
    /// enters the ledger and its consumers cascade to the pushed staging tip.
    Mainline,
    /// Coordinated cross-branch verify over an already-staged set: read a
    /// component at its staging-branch tip where that branch exists (its
    /// mainline tip otherwise), and seed the cascade ledger with those existing
    /// staging tips — so a consumer resolves an already-staged producer to its
    /// staging tip rather than its mainline, and the whole staged set verifies
    /// together. The same cascade rule, pin models, and verify then apply.
    StagedCascade,
}

/// The injected boundaries of one run.
pub struct RunBoundaries {
    pub repository_opener: Box<dyn RepositoryOpener>,
    pub nar_hash_source: Box<dyn NarHashSource>,
    pub builder_host_resolver: Box<dyn BuilderHostResolver>,
    pub verifier_source: Box<dyn VerifierSource>,
    pub lock_resolver: Box<dyn TransitiveLockResolver>,
}

/// One synchronizer run.
pub struct SynchronizerRun {
    config: SynchronizerConfig,
    boundaries: RunBoundaries,
    base_selection: BaseSelection,
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
    /// Compose a run from configuration and the production boundaries. Every
    /// project-specific choice — branch scheme, commit author, builder-host
    /// strategy, verify policy — is read from `config`; none is hard-coded.
    pub fn new(config: SynchronizerConfig) -> Self {
        let repository_opener = Box::new(GitRepositoryOpener::from_path_environment(
            config.branch_scheme().clone(),
            config.commit_author().clone(),
        ));
        let builder_host_resolver = Box::new(ConfiguredBuilderHost::new(
            config.builder_resolution().clone(),
        ));
        let verifier_source = Box::new(BuildVerifierSource::new(config.verify_policy().clone()));
        Self::with_boundaries(
            config,
            RunBoundaries {
                repository_opener,
                nar_hash_source: Box::new(NixFlakePrefetch::from_path_environment()),
                builder_host_resolver,
                verifier_source,
                lock_resolver: Box::new(CargoUpdatePrecise::from_path_environment()),
            },
        )
    }

    /// Compose a run with explicit boundaries (fixture surface for ascent
    /// witnesses). Defaults to [`BaseSelection::Mainline`]; a coordinated
    /// cross-branch run selects [`Self::with_base_selection`].
    pub fn with_boundaries(config: SynchronizerConfig, boundaries: RunBoundaries) -> Self {
        Self {
            config,
            boundaries,
            base_selection: BaseSelection::Mainline,
        }
    }

    /// Select which branch each component's base manifests are read from and
    /// how the cascade ledger starts (see [`BaseSelection`]).
    pub fn with_base_selection(mut self, base_selection: BaseSelection) -> Self {
        self.base_selection = base_selection;
        self
    }

    /// The component-name field run-scoped failures (builder-host
    /// resolution) are recorded under: the tool's own name, since these
    /// failures belong to no configured component. This is tool identity,
    /// not project data.
    fn run_scope_component() -> ComponentName {
        ComponentName::new(env!("CARGO_PKG_NAME"))
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
        // The producers already staged this run (StagedCascade): a component
        // read at its staging tip. Their tips pre-seed the cascade ledger so a
        // consumer resolves them to the staging branch, not the mainline.
        let mut prestaged: BTreeMap<ComponentName, CommitIdentifier> = BTreeMap::new();

        // 1–3: open each clone, query the base tip (mainline, or the staging
        // tip where it exists under StagedCascade), fetch it, and read the
        // manifests at it. A component that fails here is reported and excluded
        // from the ascent; the run keeps going.
        for component in self.config.components() {
            let name = component.name().clone();
            match self.load_component(&name) {
                Ok((loaded_component, staging_tip)) => {
                    if let Some(tip) = staging_tip {
                        prestaged.insert(name.clone(), tip);
                    }
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
        let mut resolver = VersionResolver::new(main_tips, self.config.branch_scheme().clone());
        // Pre-seed the cascade ledger with the already-staged producers, so a
        // consumer resolves each of them to its staging tip (the staging
        // branch) rather than its mainline. The ledger does not care whether a
        // tip was pushed by this run's ascent or by a prior coordinated step —
        // only that the producer's aligned truth this run is its staging tip.
        for (name, tip) in &prestaged {
            resolver.record_bump(name.clone(), tip.clone());
        }

        // Builder-host resolution happens once; its failure never stops bumps
        // and pushes — every verification then reports NotAttempted (§9).
        let verifier: Option<Box<dyn Verifier>> =
            match self.boundaries.builder_host_resolver.resolve() {
                Ok(host) => match self.boundaries.verifier_source.bind(host) {
                    Ok(verifier) => Some(verifier),
                    Err(error) => {
                        failures.push(Failure::new(
                            Self::run_scope_component(),
                            FailureStage::RoleResolution,
                            FailureDetail::new(error.to_string()),
                        ));
                        None
                    }
                },
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

    /// Open a component and read its base manifests. Returns the loaded
    /// component and, under [`BaseSelection::StagedCascade`], the staging tip
    /// it was read at (`None` when it was read at its mainline tip — either the
    /// normal mode, or a component with no staging branch).
    fn load_component(
        &self,
        name: &ComponentName,
    ) -> Result<(LoadedComponent, Option<CommitIdentifier>), Error> {
        let clone_path = self.config.checkout_path(name)?;
        let remote_url = self.config.repository_url(name)?;
        let repository = self
            .boundaries
            .repository_opener
            .open(name, clone_path, remote_url)?;
        // In StagedCascade a component read at its staging tip is a pre-staged
        // producer; one with no staging branch falls back to its mainline tip
        // and stays out of the ledger (it resolves to its mainline).
        let staging_tip = match self.base_selection {
            BaseSelection::Mainline => None,
            BaseSelection::StagedCascade => repository.remote_staging_tip()?,
        };
        let tip = match &staging_tip {
            Some(tip) => tip.clone(),
            None => repository.remote_main_tip()?,
        };
        repository.fetch(&tip)?;
        let manifests = ComponentManifests::load_at(repository.as_ref(), name, tip)?;
        Ok((
            LoadedComponent {
                repository,
                manifests,
            },
            staging_tip,
        ))
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
                        PushedBranch::new(self.config.branch_scheme().staging().clone(), tip),
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
                        let package = self.producer_package_name(name, &producer, loaded);
                        lock_gap = Some((package, target.revision().clone()));
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
            ResolvedTarget::SynchronizerTip(_) => {
                GitReference::Branch(self.config.branch_scheme().staging().clone())
            }
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
        let next_branch = target.reachable_branch(self.config.branch_scheme());
        let previous = cargo
            .manifest_mut()
            .redirect_git_dependency(name, GitReference::Branch(next_branch.clone()))?;
        let previous_value = match previous {
            GitReference::Branch(branch) => PinValue::Reference(branch),
            GitReference::Tag(tag) => PinValue::Reference(BranchName::new(tag)),
            GitReference::Revision(revision) => PinValue::Revision(revision),
            GitReference::DefaultBranch => {
                PinValue::Reference(self.config.branch_scheme().mainline().clone())
            }
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

    /// The Cargo package identity `cargo update -p <package>` must address for
    /// the fallback: the name the *producer* publishes, read from its root
    /// `[package] name` at the target revision.
    ///
    /// The consumer's recorded pin name is the repo/table key, which need not
    /// be the package identity — a `nota-next`-keyed pin (or a lock entry left
    /// from before the producer dropped `-next` from its crate names) resolves
    /// to the package `nota`. Edge discovery matches producers by git-URL
    /// repository identity, never by this name (ARCHITECTURE.md §4); the
    /// fallback's `-p` spec is the one place a package identity is required, so
    /// it comes from the producer's own manifest, not the consumer's key.
    /// Passing the repo/table key yields `no matching package named <key>` and
    /// leaves the typed-edited lock — invalid at the new revision — committed.
    ///
    /// A producer whose root manifest publishes no package (a virtual
    /// workspace) cannot answer for the identity; there the consumer's recorded
    /// name is the best available spec and the workspace member's own crate
    /// name.
    fn producer_package_name(
        &self,
        recorded: &DependencyName,
        producer: &ComponentName,
        loaded: &BTreeMap<ComponentName, LoadedComponent>,
    ) -> DependencyName {
        loaded
            .get(producer)
            .and_then(|component| component.manifests.cargo())
            .and_then(|cargo| cargo.manifest().package_name())
            .cloned()
            .unwrap_or_else(|| recorded.clone())
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
