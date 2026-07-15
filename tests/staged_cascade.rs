//! Coordinated cross-branch staged-verify witness.
//!
//! Models the rename-migration flow: a set of producers is already staged on a
//! staging branch (`main`) with rewritten identity, and a multi-level
//! consumer staged there still declares its producers `branch = "main"`. Under
//! `BaseSelection::StagedCascade` the run reads each component at its staging
//! tip where it exists, seeds the cascade ledger with those tips, and the
//! existing cascade repins the consumer's producer pins onto the staging
//! branch — so the whole staged set verifies together instead of a consumer
//! reintroducing a producer's un-rewritten mainline.
//!
//!   nota  (leaf, not staged: read at main; already publishes the new name)
//!   schema      (staged on main; its own nota pin already aligned to main)
//!   schema-rust (staged on main; pins schema at schema@main — must cascade
//!                to schema@main, or an isolated verify pulls schema@main)
//!
//! The staging branch here is `main`, not `synchronizer` — the branch
//! names are configuration, so the same generic cascade drives any scheme.

mod fixtures;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use fixtures::{
    FixtureBuilderHost, FixtureOpener, FixturePrefetch, FixtureRepository, FixtureVerifierSource,
    UnreachableLockResolver, revision, standard_config_with_scheme,
};
use synchronizer::configuration::{BranchScheme, Component, ComponentCheckout};
use synchronizer::driver::{BaseSelection, RunBoundaries, SynchronizerRun};
use synchronizer::report::{Action, PinValue, Verification, VerificationGate};
use synchronizer::topology::PinLayer;
use synchronizer::types::{BranchName, BuilderHost, ComponentName};

fn nota_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"nota\"\n",
                "version = \"0.5.1\"\n",
                "\n",
                "[dependencies]\n",
            )
            .to_string(),
        ),
        (
            "Cargo.lock".to_string(),
            concat!(
                "version = 4\n",
                "\n",
                "[[package]]\n",
                "name = \"nota\"\n",
                "version = \"0.5.1\"\n",
            )
            .to_string(),
        ),
    ])
}

fn schema_drop_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"schema\"\n",
                "version = \"0.2.0\"\n",
                "\n",
                "[dependencies]\n",
                "nota = { package = \"nota\", git = \"https://github.com/LiGoldragon/nota.git\", branch = \"main\" }\n",
            )
            .to_string(),
        ),
        (
            "Cargo.lock".to_string(),
            format!(
                concat!(
                    "version = 4\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"nota\"\n",
                    "version = \"0.5.1\"\n",
                    "source = \"git+https://github.com/LiGoldragon/nota.git?branch=main#{nota_main}\"\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"schema\"\n",
                    "version = \"0.2.0\"\n",
                    "dependencies = [\n \"nota\",\n]\n",
                ),
                nota_main = revision("nota-main").as_str()
            ),
        ),
    ])
}

fn schema_rust_drop_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"schema-rust\"\n",
                "version = \"0.5.3\"\n",
                "\n",
                "[dependencies]\n",
                "nota = { package = \"nota\", git = \"https://github.com/LiGoldragon/nota.git\", branch = \"main\" }\n",
                "schema = { package = \"schema\", git = \"https://github.com/LiGoldragon/schema.git\", branch = \"main\" }\n",
            )
            .to_string(),
        ),
        (
            "Cargo.lock".to_string(),
            format!(
                concat!(
                    "version = 4\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"nota\"\n",
                    "version = \"0.5.1\"\n",
                    "source = \"git+https://github.com/LiGoldragon/nota.git?branch=main#{nota_main}\"\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"schema\"\n",
                    "version = \"0.2.0\"\n",
                    "source = \"git+https://github.com/LiGoldragon/schema.git?branch=main#{schema_main}\"\n",
                    "dependencies = [\n \"nota\",\n]\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"schema-rust\"\n",
                    "version = \"0.5.3\"\n",
                    "dependencies = [\n \"nota\",\n \"schema\",\n]\n",
                ),
                nota_main = revision("nota-main").as_str(),
                schema_main = revision("schema-main").as_str()
            ),
        ),
    ])
}

#[test]
fn a_staged_consumer_cascades_its_producers_onto_the_staging_branch() {
    let nota = Rc::new(FixtureRepository::new(
        "nota",
        revision("nota-main"),
        nota_files(),
    ));
    // schema and schema-rust are already staged on `main`. Their `main`
    // trees are never read under StagedCascade; the staging tree is authoritative.
    let schema = Rc::new(
        FixtureRepository::new("schema", revision("schema-main"), schema_drop_files())
            .with_staging(revision("schema-drop"), schema_drop_files()),
    );
    let schema_rust = Rc::new(
        FixtureRepository::new(
            "schema-rust",
            revision("schema-rust-main"),
            schema_rust_drop_files(),
        )
        .with_staging(revision("schema-rust-drop"), schema_rust_drop_files()),
    );
    let opener = FixtureOpener {
        repositories: BTreeMap::from([
            (ComponentName::new("nota"), Rc::clone(&nota)),
            (ComponentName::new("schema"), Rc::clone(&schema)),
            (ComponentName::new("schema-rust"), Rc::clone(&schema_rust)),
        ]),
    };
    let verified = Rc::new(RefCell::new(Vec::new()));
    let config = standard_config_with_scheme(
        vec![
            Component::new(ComponentName::new("nota"), ComponentCheckout::AtRoot),
            Component::new(ComponentName::new("schema"), ComponentCheckout::AtRoot),
            Component::new(ComponentName::new("schema-rust"), ComponentCheckout::AtRoot),
        ],
        BranchScheme::new(BranchName::new("main"), BranchName::new("main")),
    );
    let run = SynchronizerRun::with_boundaries(
        config,
        RunBoundaries {
            repository_opener: Box::new(opener),
            nar_hash_source: Box::new(FixturePrefetch),
            builder_host_resolver: Box::new(FixtureBuilderHost {
                host: BuilderHost::new("prometheus"),
            }),
            verifier_source: Box::new(FixtureVerifierSource {
                verified: Rc::clone(&verified),
            }),
            lock_resolver: Box::new(UnreachableLockResolver {
                witness: "staged-cascade witness",
            }),
        },
    )
    .with_base_selection(BaseSelection::StagedCascade);

    let report = run.execute().expect("the staged cascade completes");
    assert!(
        !report.has_failures(),
        "collected failures: {:?}",
        report.failures()
    );
    let levels = report.levels();
    assert_eq!(levels.len(), 3);

    // Level 0: nota is a non-staged leaf, read at main, already publishing the
    // new name — nothing to do, and never pushed.
    let nota_outcome = &levels[0].repositories()[0];
    assert_eq!(nota_outcome.component(), &ComponentName::new("nota"));
    assert_eq!(nota_outcome.action(), &Action::AlreadyAligned);
    assert!(nota.pushed.borrow().is_empty());

    // Level 1: schema is staged, and its own nota pin already reaches nota's
    // main tip, so it is already aligned — the run does not re-push a producer
    // it only reads.
    let schema_outcome = &levels[1].repositories()[0];
    assert_eq!(schema_outcome.component(), &ComponentName::new("schema"));
    assert_eq!(schema_outcome.action(), &Action::AlreadyAligned);
    assert!(
        schema.pushed.borrow().is_empty(),
        "an already-aligned staged producer is read, not re-pushed"
    );

    // Level 2: schema-rust cascades. Its schema pin resolves to schema's
    // *staging* tip (the ledger seeded from the pre-staged set). Because this
    // fixture deliberately names the same branch for mainline and staging,
    // the manifest already reaches that branch and needs no no-op rewrite;
    // the lock still repins to the staged commit. Its nota pin stays untouched.
    let schema_rust_outcome = &levels[2].repositories()[0];
    let Action::Bumped(bump) = schema_rust_outcome.action() else {
        panic!("schema-rust must cascade: {schema_rust_outcome:?}");
    };
    let schema_name = ComponentName::new("schema");
    assert!(
        bump.applied()
            .iter()
            .all(|applied| applied.layer() != PinLayer::CargoManifest),
        "an already-reachable staging branch must not receive a no-op manifest edit"
    );
    let schema_lock_bump = bump
        .applied()
        .iter()
        .find(|applied| {
            applied.layer() == PinLayer::CargoLock && applied.dependency() == &schema_name
        })
        .expect("schema-rust's schema lock pin moves to the staging tip");
    assert_eq!(
        schema_lock_bump.next(),
        &PinValue::Revision(revision("schema-drop")),
        "the schema lock repins to schema's pushed staging tip, not schema@main"
    );
    // The nota pin was never touched: a non-staged producer stays on main.
    assert!(
        bump.applied()
            .iter()
            .all(|applied| applied.dependency() != &ComponentName::new("nota")),
        "the non-staged nota pin stays on its mainline"
    );

    // The committed consumer tree is self-consistent for an isolated verify:
    // schema now points at the staging branch, nota stays on main.
    let tip = bump.pushed().tip().clone();
    assert_eq!(bump.pushed().branch(), &BranchName::new("main"));
    let committed_manifest = schema_rust
        .file_text(&tip, "Cargo.toml")
        .expect("the cascade commit carries the manifest");
    assert!(
        committed_manifest.contains(
            "schema = { package = \"schema\", git = \"https://github.com/LiGoldragon/schema.git\", branch = \"main\" }"
        ),
        "schema is redirected to main: {committed_manifest}"
    );
    assert!(
        committed_manifest.contains(
            "nota = { package = \"nota\", git = \"https://github.com/LiGoldragon/nota.git\", branch = \"main\" }"
        ),
        "nota stays on main: {committed_manifest}"
    );
    let committed_lock = schema_rust
        .file_text(&tip, "Cargo.lock")
        .expect("the cascade commit carries the lock");
    assert!(
        committed_lock.contains(&format!(
            "?branch=main#{}",
            revision("schema-drop").as_str()
        )),
        "the schema lock entry now resolves against main: {committed_lock}"
    );
    assert!(
        committed_lock.contains(&format!("?branch=main#{}", revision("nota-main").as_str())),
        "the nota lock entry still resolves against main: {committed_lock}"
    );

    // Only the cascading consumer was pushed and verified — the producers were
    // read, never written.
    assert_eq!(schema_rust.pushed.borrow().len(), 1);
    assert_eq!(
        schema_rust_outcome.verification(),
        &Verification::Verified(BuilderHost::new("prometheus"), VerificationGate::WireChecks)
    );
    assert_eq!(
        verified.borrow().as_slice(),
        &[(ComponentName::new("schema-rust"), tip)]
    );
}
