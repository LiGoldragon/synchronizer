//! Shared in-memory fixtures for the synchronizer witnesses.
//!
//! A [`FixtureRepository`] is a component's git surface backed by maps: file
//! trees per revision, a synthetic commit builder, and a recorded push log.
//! No network, no object store, no working copy.
//!
//! Each integration test binary compiles this module and uses its own
//! subset, so unused-item lints are quieted here.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use synchronizer::build_verify::{Verifier, VerifyPolicy, WireCheckWord};
use synchronizer::configuration::{
    BranchScheme, BuilderResolution, ClusterSource, CommitAuthor, Component, Forge, ForgeOwner,
    SynchronizerConfig,
};
use synchronizer::driver::{BuilderHostResolver, RepositoryOpener, VerifierSource};
use synchronizer::error::Error;
use synchronizer::flake_lock::{NarHashSource, PinnedFlakeReference, PrefetchedSource};
use synchronizer::git_repository::{
    CommitMessage, ComponentRepository, FileEdit, RepositoryFilePath, TreeFile,
};
use synchronizer::transitive_lock::{TransitiveLockRequest, TransitiveLockResolver};
use synchronizer::types::{
    AbsolutePath, AuthorEmail, AuthorName, BranchName, BuilderHost, BuilderRole, CommitIdentifier,
    ComponentName, NarHash, RepositoryUrl, TomlText,
};

/// The standard criome-shaped test configuration used by the ascent,
/// topology, and resolver witnesses: `LiGoldragon` forge, `main`/`synchronizer`
/// branch scheme, cluster-role builder resolution, wire-exercising verify.
/// The generic (non-criome) paths are witnessed separately in
/// `tests/configuration.rs` and `tests/driver.rs`.
pub fn standard_config(components: Vec<Component>) -> SynchronizerConfig {
    SynchronizerConfig::new(
        Forge::GitHub(ForgeOwner::new("LiGoldragon")),
        AbsolutePath::new("/git/github.com/LiGoldragon"),
        components,
        BranchScheme::new(BranchName::new("main"), BranchName::new("synchronizer")),
        BuilderResolution::ClusterRole(
            BuilderRole::new("NixBuilder"),
            ClusterSource::ClusterProposal(AbsolutePath::new("/cluster/datom.nota")),
        ),
        VerifyPolicy::WireExercising(
            ["daemon", "daemons", "socket", "sockets", "wire"]
                .iter()
                .map(|word| WireCheckWord::new(*word))
                .collect(),
        ),
        CommitAuthor::new(
            AuthorName::new("synchronizer"),
            AuthorEmail::new("noreply@example.net"),
        ),
    )
}

/// A synthetic 40-hex revision from a short tag, so fixture revisions are
/// stable and readable in assertions.
pub fn revision(tag: &str) -> CommitIdentifier {
    let mut text = String::new();
    for byte in tag.bytes() {
        text.push_str(&format!("{byte:02x}"));
    }
    while text.len() < 40 {
        text.push('0');
    }
    CommitIdentifier::new(&text[..40])
}

/// One component's in-memory git surface.
pub struct FixtureRepository {
    pub component: ComponentName,
    pub main_tip: CommitIdentifier,
    pub trees: RefCell<BTreeMap<String, BTreeMap<String, String>>>,
    pub pushed: RefCell<Vec<CommitIdentifier>>,
    pub commit_counter: RefCell<u32>,
}

impl FixtureRepository {
    pub fn new(
        component: &str,
        main_tip: CommitIdentifier,
        files: BTreeMap<String, String>,
    ) -> Self {
        let mut trees = BTreeMap::new();
        trees.insert(main_tip.as_str().to_string(), files);
        Self {
            component: ComponentName::new(component),
            main_tip,
            trees: RefCell::new(trees),
            pushed: RefCell::new(Vec::new()),
            commit_counter: RefCell::new(0),
        }
    }

    pub fn file_text(&self, revision: &CommitIdentifier, path: &str) -> Option<String> {
        self.trees
            .borrow()
            .get(revision.as_str())
            .and_then(|tree| tree.get(path))
            .cloned()
    }
}

impl ComponentRepository for FixtureRepository {
    fn remote_main_tip(&self) -> Result<CommitIdentifier, Error> {
        Ok(self.main_tip.clone())
    }

    fn fetch(&self, _revision: &CommitIdentifier) -> Result<(), Error> {
        Ok(())
    }

    fn file_at(
        &self,
        revision: &CommitIdentifier,
        path: &RepositoryFilePath,
    ) -> Result<Option<String>, Error> {
        Ok(self
            .trees
            .borrow()
            .get(revision.as_str())
            .and_then(|tree| tree.get(path.as_str()))
            .cloned())
    }

    fn tree_files_at(&self, revision: &CommitIdentifier) -> Result<Vec<TreeFile>, Error> {
        Ok(self
            .trees
            .borrow()
            .get(revision.as_str())
            .map(|tree| {
                tree.iter()
                    .map(|(path, content)| TreeFile {
                        path: RepositoryFilePath::new(path.clone()),
                        content: content.as_bytes().to_vec(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn commit_file_edits(
        &self,
        base: &CommitIdentifier,
        edits: &[FileEdit],
        _message: &CommitMessage,
    ) -> Result<CommitIdentifier, Error> {
        let mut counter = self.commit_counter.borrow_mut();
        *counter += 1;
        let tip = revision(&format!("{}-sync-{}", self.component.as_str(), counter));
        let mut trees = self.trees.borrow_mut();
        let mut tree = trees.get(base.as_str()).cloned().unwrap_or_default();
        for edit in edits {
            tree.insert(edit.path().as_str().to_string(), edit.content().to_string());
        }
        trees.insert(tip.as_str().to_string(), tree);
        Ok(tip)
    }

    fn push_synchronizer_branch(&self, commit: &CommitIdentifier) -> Result<(), Error> {
        self.pushed.borrow_mut().push(commit.clone());
        Ok(())
    }
}

/// Hands out shared fixture repositories by component name.
pub struct FixtureOpener {
    pub repositories: BTreeMap<ComponentName, Rc<FixtureRepository>>,
}

/// An `Rc` view of one fixture repository, so the test keeps a handle while
/// the driver owns a box.
pub struct SharedRepository(pub Rc<FixtureRepository>);

impl ComponentRepository for SharedRepository {
    fn remote_main_tip(&self) -> Result<CommitIdentifier, Error> {
        self.0.remote_main_tip()
    }

    fn fetch(&self, revision: &CommitIdentifier) -> Result<(), Error> {
        self.0.fetch(revision)
    }

    fn file_at(
        &self,
        revision: &CommitIdentifier,
        path: &RepositoryFilePath,
    ) -> Result<Option<String>, Error> {
        self.0.file_at(revision, path)
    }

    fn tree_files_at(&self, revision: &CommitIdentifier) -> Result<Vec<TreeFile>, Error> {
        self.0.tree_files_at(revision)
    }

    fn commit_file_edits(
        &self,
        base: &CommitIdentifier,
        edits: &[FileEdit],
        message: &CommitMessage,
    ) -> Result<CommitIdentifier, Error> {
        self.0.commit_file_edits(base, edits, message)
    }

    fn push_synchronizer_branch(&self, commit: &CommitIdentifier) -> Result<(), Error> {
        self.0.push_synchronizer_branch(commit)
    }
}

impl RepositoryOpener for FixtureOpener {
    fn open(
        &self,
        component: &ComponentName,
        _clone_path: PathBuf,
        _remote_url: RepositoryUrl,
    ) -> Result<Box<dyn ComponentRepository>, Error> {
        let repository = self
            .repositories
            .get(component)
            .ok_or_else(|| Error::UnknownComponent(component.clone()))?;
        Ok(Box::new(SharedRepository(Rc::clone(repository))))
    }
}

/// A deterministic narHash source: no network, stable hashes derived from
/// the reference.
pub struct FixturePrefetch;

impl NarHashSource for FixturePrefetch {
    fn prefetch(&self, reference: &PinnedFlakeReference) -> Result<PrefetchedSource, Error> {
        let hash = format!(
            "sha256-fixture-{}",
            reference.to_flake_reference().as_str().len()
        );
        Ok(PrefetchedSource::new(NarHash::new(hash), 1_750_000_000))
    }
}

/// A fixture verifier: records every verified revision, always green.
pub struct FixtureVerifier {
    pub host: BuilderHost,
    pub verified: Rc<RefCell<Vec<(ComponentName, CommitIdentifier)>>>,
}

impl Verifier for FixtureVerifier {
    fn host(&self) -> &BuilderHost {
        &self.host
    }

    fn verify(
        &self,
        _forge: &Forge,
        component: &ComponentName,
        revision: &CommitIdentifier,
    ) -> synchronizer::build_verify::VerificationOutcome {
        self.verified
            .borrow_mut()
            .push((component.clone(), revision.clone()));
        synchronizer::build_verify::VerificationOutcome::Verified(
            synchronizer::report::VerificationGate::WireChecks,
        )
    }
}

/// Binds the fixture verifier to the resolved host.
pub struct FixtureVerifierSource {
    pub verified: Rc<RefCell<Vec<(ComponentName, CommitIdentifier)>>>,
}

impl VerifierSource for FixtureVerifierSource {
    fn bind(&self, host: BuilderHost) -> Result<Box<dyn Verifier>, Error> {
        Ok(Box::new(FixtureVerifier {
            host,
            verified: Rc::clone(&self.verified),
        }))
    }
}

/// A builder-host resolver answering with a fixed host — the ascent
/// witnesses do not exercise cluster-proposal decoding (role_resolution
/// witnesses do). Stands in for the production `ConfiguredBuilderHost`.
pub struct FixtureBuilderHost {
    pub host: BuilderHost,
}

impl BuilderHostResolver for FixtureBuilderHost {
    fn resolve(&self) -> Result<BuilderHost, Error> {
        Ok(self.host.clone())
    }
}

/// A transitive-lock resolver that must never be needed: reaching it fails
/// the witness loudly.
pub struct UnreachableLockResolver {
    pub witness: &'static str,
}

impl TransitiveLockResolver for UnreachableLockResolver {
    fn resolve_lock(&self, request: &TransitiveLockRequest) -> Result<TomlText, Error> {
        panic!(
            "{}: transitive-lock fallback unexpectedly invoked for {}",
            self.witness,
            request.consumer.as_str()
        );
    }
}
