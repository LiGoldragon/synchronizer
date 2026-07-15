//! Release-train P0/P2 contract witnesses.

use std::collections::{BTreeMap, BTreeSet};

use synchronizer::release_train::{
    CandidateSelector, ComponentLockIdentity, ImmutableExternal, NixSourceAttestation,
    ReleaseTrainError, ReleaseTrainIntent, ReleaseTrainName, ReleaseTrainResolution,
    ResolvedSelector, TrainComponent,
};
use synchronizer::types::{CommitIdentifier, ComponentName, NarHash};

struct TrainFixture;

impl TrainFixture {
    fn commit(character: char) -> CommitIdentifier {
        CommitIdentifier::new(character.to_string().repeat(40))
    }

    fn component(name: &str) -> ComponentName {
        ComponentName::new(name)
    }

    fn intent() -> ReleaseTrainIntent {
        ReleaseTrainIntent::new(
            ReleaseTrainName::new("language-family-poc"),
            vec![
                TrainComponent::new(
                    Self::component("nota"),
                    CandidateSelector::Branch(synchronizer::types::BranchName::new("next-gen")),
                    Self::commit('a'),
                ),
                TrainComponent::new(
                    Self::component("schema-language"),
                    CandidateSelector::Branch(synchronizer::types::BranchName::new(
                        "poc-structural",
                    )),
                    Self::commit('b'),
                ),
                TrainComponent::new(
                    Self::component("schema-rust"),
                    CandidateSelector::Mainline,
                    Self::commit('c'),
                ),
            ],
            vec![ImmutableExternal::new(
                Self::component("signal-frame"),
                Self::commit('d'),
            )],
        )
    }

    fn selectors() -> Vec<ResolvedSelector> {
        vec![
            ResolvedSelector::new(
                Self::component("nota"),
                Self::commit('e'),
                Self::commit('a'),
                Self::commit('1'),
            ),
            ResolvedSelector::new(
                Self::component("schema-language"),
                Self::commit('f'),
                Self::commit('b'),
                Self::commit('2'),
            ),
            ResolvedSelector::new(
                Self::component("schema-rust"),
                Self::commit('3'),
                Self::commit('c'),
                Self::commit('3'),
            ),
        ]
    }

    fn attestations() -> Vec<NixSourceAttestation> {
        vec![
            NixSourceAttestation::new(
                Self::component("nota"),
                Self::commit('1'),
                NarHash::new("sha256-nota"),
            ),
            NixSourceAttestation::new(
                Self::component("schema-language"),
                Self::commit('2'),
                NarHash::new("sha256-schema-language"),
            ),
            NixSourceAttestation::new(
                Self::component("schema-rust"),
                Self::commit('3'),
                NarHash::new("sha256-schema-rust"),
            ),
        ]
    }

    fn locks() -> Vec<ComponentLockIdentity> {
        vec![
            ComponentLockIdentity::from_text(Self::component("nota"), "nota cargo", "nota flake"),
            ComponentLockIdentity::from_text(
                Self::component("schema-language"),
                "schema language cargo",
                "schema language flake",
            ),
            ComponentLockIdentity::from_text(
                Self::component("schema-rust"),
                "schema rust cargo",
                "schema rust flake",
            ),
        ]
    }

    fn resolution() -> ReleaseTrainResolution {
        ReleaseTrainResolution::new(
            Self::intent(),
            Self::selectors(),
            Self::attestations(),
            Self::locks(),
            BTreeSet::from([
                Self::component("nota"),
                Self::component("schema-language"),
                Self::component("schema-rust"),
            ]),
            BTreeMap::from([(Self::component("signal-frame"), Self::commit('d'))]),
        )
    }
}

#[test]
fn authored_language_family_intent_is_an_independent_nota_document() {
    let intent = ReleaseTrainIntent::from_nota_text(include_str!(
        "../release-trains/language-family-poc.nota"
    ))
    .expect("seed intent decodes");
    assert_eq!(intent.name().as_str(), "language-family-poc");
    assert_eq!(intent.components().len(), 3);
    assert_eq!(intent.components()[0].component().as_str(), "nota");
}

#[test]
fn release_train_closure_is_canonical_and_contains_only_immutable_nix_sources() {
    let closure = TrainFixture::resolution()
        .resolve()
        .expect("valid train resolves");
    assert_eq!(
        closure.candidate_branch().as_str(),
        "train/language-family-poc"
    );
    let json = closure.to_canonical_json().expect("JSON projection");
    assert!(
        !json.contains("/tmp"),
        "projection must not capture local paths: {json}"
    );
    assert!(json.contains("schema-language"));
    let flake = closure.to_integration_flake("LiGoldragon");
    assert!(flake.contains("github:LiGoldragon/nota/1111111111111111111111111111111111111111"));
    assert!(flake.contains("narHash = \"sha256-nota\""));
    assert!(
        !flake.contains("path:"),
        "integration flake must be portable: {flake}"
    );
    let repeat = TrainFixture::resolution()
        .resolve()
        .expect("same closure resolves");
    assert_eq!(
        closure.identity(),
        repeat.identity(),
        "same inputs have one closure identity"
    );
}

#[test]
fn undeclared_internal_component_is_a_loud_train_failure() {
    let mut members = BTreeSet::from([
        TrainFixture::component("nota"),
        TrainFixture::component("schema-language"),
        TrainFixture::component("schema-rust"),
    ]);
    members.insert(TrainFixture::component("unplanned-component"));
    let resolution = ReleaseTrainResolution::new(
        TrainFixture::intent(),
        TrainFixture::selectors(),
        TrainFixture::attestations(),
        TrainFixture::locks(),
        members,
        BTreeMap::from([(
            TrainFixture::component("signal-frame"),
            TrainFixture::commit('d'),
        )]),
    );
    assert_eq!(
        resolution.resolve(),
        Err(ReleaseTrainError::UndeclaredInternalEdge(
            TrainFixture::component("unplanned-component")
        )),
    );
}

#[test]
fn external_component_requires_exact_immutable_admission() {
    let resolution = ReleaseTrainResolution::new(
        TrainFixture::intent(),
        TrainFixture::selectors(),
        TrainFixture::attestations(),
        TrainFixture::locks(),
        BTreeSet::from([
            TrainFixture::component("nota"),
            TrainFixture::component("schema-language"),
            TrainFixture::component("schema-rust"),
        ]),
        BTreeMap::from([(
            TrainFixture::component("signal-frame"),
            TrainFixture::commit('9'),
        )]),
    );
    assert_eq!(
        resolution.resolve(),
        Err(ReleaseTrainError::UnadmittedExternal {
            component: TrainFixture::component("signal-frame"),
            commit: TrainFixture::commit('9'),
        }),
    );
}

#[test]
fn moved_expected_base_is_a_loud_selector_failure() {
    let mut selectors = TrainFixture::selectors();
    selectors[0] = ResolvedSelector::new(
        TrainFixture::component("nota"),
        TrainFixture::commit('e'),
        TrainFixture::commit('9'),
        TrainFixture::commit('1'),
    );
    let resolution = ReleaseTrainResolution::new(
        TrainFixture::intent(),
        selectors,
        TrainFixture::attestations(),
        TrainFixture::locks(),
        BTreeSet::from([
            TrainFixture::component("nota"),
            TrainFixture::component("schema-language"),
            TrainFixture::component("schema-rust"),
        ]),
        BTreeMap::from([(
            TrainFixture::component("signal-frame"),
            TrainFixture::commit('d'),
        )]),
    );
    assert_eq!(
        resolution.resolve(),
        Err(ReleaseTrainError::ExpectedBaseMoved {
            component: TrainFixture::component("nota"),
            expected: TrainFixture::commit('a'),
            observed: TrainFixture::commit('9'),
        }),
    );
}
