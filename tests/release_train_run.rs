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
    CandidateSelector, ComponentLockIdentity, NixSourceAttestation, ReleaseTrainIntent,
    ReleaseTrainName, TrainComponent,
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
    let closure = materialized
        .resolve_closure(attestations, locks, members, BTreeMap::new())
        .expect("materialized candidates emit an immutable closure");
    let artifact_directory = tempfile::tempdir().expect("artifact directory");
    let artifacts = closure
        .write_integration_artifacts(artifact_directory.path(), "LiGoldragon")
        .expect("portable Nix artifacts emit");
    let generated_flake = std::fs::read_to_string(artifacts.flake_path()).expect("generated flake");
    assert!(generated_flake.contains("github:LiGoldragon/nota/"));
    assert!(!generated_flake.contains("path:"));
}
