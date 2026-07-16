//! Synthetic remote witness for live release-train selector and branch flow.

mod fixtures;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use fixtures::{
    FixtureBuilderHost, FixtureOpener, FixturePrefetch, FixtureRepository, FixtureVerifierSource,
    UnreachableLockResolver, revision, standard_config,
};
use synchronizer::configuration::{Component, ComponentCheckout};
use synchronizer::driver::{RunBoundaries, SynchronizerRun};
use synchronizer::release_train::{
    CandidateSelector, ComponentLockIdentity, ImmutableExternal, NixSourceAttestation,
    ReleaseTrainError, ReleaseTrainIntent, ReleaseTrainName, TrainComponent,
};
use synchronizer::types::{BranchName, BuilderHost, ComponentName, NarHash};

struct TrainRunFixture;

impl TrainRunFixture {
    fn component(name: &str) -> ComponentName {
        ComponentName::new(name)
    }

    fn train_intent() -> ReleaseTrainIntent {
        ReleaseTrainIntent::new(
            ReleaseTrainName::new("synthetic-release"),
            vec![
                TrainComponent::new(
                    Self::component("nota"),
                    CandidateSelector::Branch(BranchName::new("next-gen")),
                    revision("nota-main"),
                ),
                TrainComponent::new(
                    Self::component("schema-language"),
                    CandidateSelector::Mainline,
                    revision("schema-main"),
                ),
                TrainComponent::new(
                    Self::component("schema-rust"),
                    CandidateSelector::ExactCommit(revision("schema-rust-main")),
                    revision("schema-rust-main"),
                ),
            ],
            Vec::new(),
        )
    }
}

#[test]
fn live_train_resolution_materializes_scoped_candidate_branches_from_pushed_truth() {
    let nota = Rc::new(
        FixtureRepository::new("nota", revision("nota-main"), BTreeMap::new())
            .with_staging(revision("nota-next-gen"), BTreeMap::new()),
    );
    let schema_language = Rc::new(FixtureRepository::new(
        "schema-language",
        revision("schema-main"),
        BTreeMap::new(),
    ));
    let schema_rust = Rc::new(FixtureRepository::new(
        "schema-rust",
        revision("schema-rust-main"),
        BTreeMap::new(),
    ));
    let config = standard_config(vec![
        Component::new(
            TrainRunFixture::component("nota"),
            ComponentCheckout::AtRoot,
        ),
        Component::new(
            TrainRunFixture::component("schema-language"),
            ComponentCheckout::AtRoot,
        ),
        Component::new(
            TrainRunFixture::component("schema-rust"),
            ComponentCheckout::AtRoot,
        ),
    ]);
    let boundaries = RunBoundaries {
        repository_opener: Box::new(FixtureOpener {
            repositories: BTreeMap::from([
                (TrainRunFixture::component("nota"), Rc::clone(&nota)),
                (
                    TrainRunFixture::component("schema-language"),
                    Rc::clone(&schema_language),
                ),
                (
                    TrainRunFixture::component("schema-rust"),
                    Rc::clone(&schema_rust),
                ),
            ]),
        }),
        nar_hash_source: Box::new(FixturePrefetch),
        builder_host_resolver: Box::new(FixtureBuilderHost {
            host: BuilderHost::new("prometheus"),
        }),
        verifier_source: Box::new(FixtureVerifierSource {
            verified: Rc::new(RefCell::new(Vec::new())),
        }),
        lock_resolver: Box::new(UnreachableLockResolver {
            witness: "no dependencies in release-train fixture",
        }),
    };

    let materialized =
        SynchronizerRun::release_train(config, TrainRunFixture::train_intent(), boundaries)
            .execute()
            .expect("pushed selectors materialize a candidate train");

    assert_eq!(
        materialized.candidate_branch().as_str(),
        "train/synthetic-release"
    );
    assert_eq!(materialized.selectors().len(), 3);
    assert!(!materialized.report().has_failures());
    assert_eq!(nota.pushed.borrow().len(), 1);
    assert_eq!(schema_language.pushed.borrow().len(), 1);
    assert_eq!(schema_rust.pushed.borrow().len(), 1);

    let attestations = materialized
        .selectors()
        .iter()
        .map(|selector| {
            NixSourceAttestation::new(
                selector.component().clone(),
                selector.candidate().clone(),
                NarHash::new(format!("sha256-{}", selector.component().as_str())),
            )
        })
        .collect();
    let locks = materialized
        .selectors()
        .iter()
        .map(|selector| {
            ComponentLockIdentity::from_text(
                selector.component().clone(),
                "synthetic Cargo.lock",
                "synthetic flake.lock",
            )
        })
        .collect();
    let members = materialized
        .selectors()
        .iter()
        .map(|selector| selector.component().clone())
        .collect();
    let closure = materialized.closure();
    assert_eq!(
        closure.components().len(),
        3,
        "normal train resolves its closure"
    );
    let closure = materialized
        .resolve_closure(attestations, locks, members, BTreeMap::new())
        .expect("the public closure validator remains available for callers");
    let artifact_directory = tempfile::tempdir().expect("artifact directory");
    let artifacts = closure
        .write_integration_artifacts(artifact_directory.path(), "LiGoldragon")
        .expect("portable Nix artifacts emit");
    let generated_flake = std::fs::read_to_string(artifacts.flake_path()).expect("generated flake");
    assert!(generated_flake.contains("nota.url = \"github:LiGoldragon/nota/"));
    assert!(!generated_flake.contains("narHash"));
    assert!(!generated_flake.contains("path:"));
}

#[test]
fn normal_train_records_the_observed_remote_base_and_rejects_a_mismatch() {
    let nota = Rc::new(FixtureRepository::new(
        "nota",
        revision("nota-main"),
        BTreeMap::new(),
    ));
    let config = standard_config(vec![Component::new(
        TrainRunFixture::component("nota"),
        ComponentCheckout::AtRoot,
    )]);
    let intent = ReleaseTrainIntent::new(
        ReleaseTrainName::new("base-proof"),
        vec![TrainComponent::new(
            TrainRunFixture::component("nota"),
            CandidateSelector::Mainline,
            revision("unexpected-base"),
        )],
        Vec::new(),
    );
    let result = SynchronizerRun::release_train(
        config,
        intent,
        RunBoundaries {
            repository_opener: Box::new(FixtureOpener {
                repositories: BTreeMap::from([(TrainRunFixture::component("nota"), nota)]),
            }),
            nar_hash_source: Box::new(FixturePrefetch),
            builder_host_resolver: Box::new(FixtureBuilderHost {
                host: BuilderHost::new("prometheus"),
            }),
            verifier_source: Box::new(FixtureVerifierSource {
                verified: Rc::new(RefCell::new(Vec::new())),
            }),
            lock_resolver: Box::new(UnreachableLockResolver {
                witness: "base validation occurs before lock resolution",
            }),
        },
    )
    .execute();
    assert!(
        matches!(
            result,
            Err(ReleaseTrainError::ExpectedBaseMoved { component, expected, observed })
                if component == TrainRunFixture::component("nota")
                    && expected == revision("unexpected-base")
                    && observed == revision("nota-main")
        ),
        "the observed field must be the independently queried remote main tip"
    );
}

#[test]
fn normal_train_rejects_an_owned_dependency_omitted_from_the_intent() {
    let consumer = Rc::new(FixtureRepository::new(
        "consumer",
        revision("consumer-main"),
        BTreeMap::from([(
            "Cargo.toml".to_string(),
            "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\n\n[dependencies]\nunplanned = { git = \"https://github.com/LiGoldragon/unplanned.git\", branch = \"main\" }\n".to_string(),
        ), (
            "Cargo.lock".to_string(),
            format!("version = 4\n\n[[package]]\nname = \"unplanned\"\nversion = \"0.1.0\"\nsource = \"git+https://github.com/LiGoldragon/unplanned.git?branch=main#{}\"\n", revision("unplanned-main").as_str()),
        )]),
    ));
    let config = standard_config(vec![Component::new(
        TrainRunFixture::component("consumer"),
        ComponentCheckout::AtRoot,
    )]);
    let intent = ReleaseTrainIntent::new(
        ReleaseTrainName::new("undeclared-proof"),
        vec![TrainComponent::new(
            TrainRunFixture::component("consumer"),
            CandidateSelector::Mainline,
            revision("consumer-main"),
        )],
        Vec::new(),
    );
    let result = SynchronizerRun::release_train(
        config,
        intent,
        RunBoundaries {
            repository_opener: Box::new(FixtureOpener {
                repositories: BTreeMap::from([(TrainRunFixture::component("consumer"), consumer)]),
            }),
            nar_hash_source: Box::new(FixturePrefetch),
            builder_host_resolver: Box::new(FixtureBuilderHost {
                host: BuilderHost::new("prometheus"),
            }),
            verifier_source: Box::new(FixtureVerifierSource {
                verified: Rc::new(RefCell::new(Vec::new())),
            }),
            lock_resolver: Box::new(UnreachableLockResolver {
                witness: "the isolated consumer has no configured producer",
            }),
        },
    )
    .execute();
    assert!(matches!(
        result,
        Err(ReleaseTrainError::UndeclaredInternalEdge(component))
            if component == ComponentName::new("unplanned")
    ));
}

fn immutable_external_run(
    externals: Vec<ImmutableExternal>,
    lock_sources: &[(&str, synchronizer::types::CommitIdentifier)],
) -> Result<synchronizer::release_train::MaterializedReleaseTrain, ReleaseTrainError> {
    let lock_entries = lock_sources
        .iter()
        .map(|(name, commit)| format!(
            "[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\nsource = \"git+https://github.com/LiGoldragon/{name}.git?branch=main#{}\"\n",
            commit.as_str()
        ))
        .collect::<Vec<_>>()
        .join("\n");
    let manifest_dependencies = lock_sources
        .iter()
        .map(|(name, _)| {
            format!(
                "{name} = {{ git = \"https://github.com/LiGoldragon/{name}.git\", branch = \"main\" }}\n"
            )
        })
        .collect::<String>();
    let consumer = Rc::new(FixtureRepository::new(
        "consumer",
        revision("consumer-main"),
        BTreeMap::from([
            (
                "Cargo.toml".to_string(),
                format!(
                    "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\n\n[dependencies]\n{manifest_dependencies}"
                ),
            ),
            (
                "Cargo.lock".to_string(),
                format!("version = 4\n\n{lock_entries}"),
            ),
        ]),
    ));
    SynchronizerRun::release_train(
        standard_config(vec![Component::new(
            ComponentName::new("consumer"),
            ComponentCheckout::AtRoot,
        )]),
        ReleaseTrainIntent::new(
            ReleaseTrainName::new("immutable-external-proof"),
            vec![TrainComponent::new(
                ComponentName::new("consumer"),
                CandidateSelector::Mainline,
                revision("consumer-main"),
            )],
            externals,
        ),
        RunBoundaries {
            repository_opener: Box::new(FixtureOpener {
                repositories: BTreeMap::from([(ComponentName::new("consumer"), consumer)]),
            }),
            nar_hash_source: Box::new(FixturePrefetch),
            builder_host_resolver: Box::new(FixtureBuilderHost {
                host: BuilderHost::new("prometheus"),
            }),
            verifier_source: Box::new(FixtureVerifierSource {
                verified: Rc::new(RefCell::new(Vec::new())),
            }),
            lock_resolver: Box::new(UnreachableLockResolver {
                witness: "the fixture has no declared train producer",
            }),
        },
    )
    .execute()
}

#[test]
fn normal_train_accepts_an_exactly_admitted_owned_locked_external() {
    let commit = revision("external-main");
    let materialized = immutable_external_run(
        vec![ImmutableExternal::new(
            ComponentName::new("external"),
            commit.clone(),
        )],
        &[("external", commit.clone())],
    )
    .expect("an exact admission moves the owned lock source outside the train");
    let closure = materialized.closure();
    assert_eq!(closure.components().len(), 1);
}

#[test]
fn normal_train_rejects_an_owned_locked_external_when_its_commit_differs() {
    let result = immutable_external_run(
        vec![ImmutableExternal::new(
            ComponentName::new("external"),
            revision("different-external"),
        )],
        &[("external", revision("external-main"))],
    );
    assert!(matches!(
        result,
        Err(ReleaseTrainError::UndeclaredInternalEdge(component))
            if component == ComponentName::new("external")
    ));
}

#[test]
fn normal_train_rejects_an_owned_locked_external_without_admission() {
    let result = immutable_external_run(Vec::new(), &[("external", revision("external-main"))]);
    assert!(matches!(
        result,
        Err(ReleaseTrainError::UndeclaredInternalEdge(component))
            if component == ComponentName::new("external")
    ));
}

#[test]
fn normal_train_requires_exact_admission_for_every_recursive_owned_lock_source() {
    let outer = revision("outer-main");
    let inner = revision("inner-main");
    let result = immutable_external_run(
        vec![ImmutableExternal::new(
            ComponentName::new("outer"),
            outer.clone(),
        )],
        &[("outer", outer), ("inner", inner)],
    );
    assert!(matches!(
        result,
        Err(ReleaseTrainError::UndeclaredInternalEdge(component))
            if component == ComponentName::new("inner")
    ));
}
