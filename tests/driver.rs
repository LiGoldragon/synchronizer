//! The ascent witness: a three-component fixture chain driven end-to-end
//! through `SynchronizerRun::execute` with in-memory boundaries.
//!
//! signal-frame (leaf, moved ahead on main) <- signal-router <- signal-harness
//!
//! Expected cascade: the frame is already aligned; the router bumps its
//! frame pins to the frame's *main* tip; the harness bumps its router pins
//! to the router's pushed *synchronizer* tip (the ledger rule) and its
//! transitive frame lock entry to the frame's main tip, redirecting its
//! router manifest declaration to the synchronizer branch.

mod fixtures;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use fixtures::{
    FixtureBuilderHost, FixtureOpener, FixturePrefetch, FixtureRepository, FixtureVerifierSource,
    UnreachableLockResolver, revision, standard_config,
};
use synchronizer::build_verify::VerifyPolicy;
use synchronizer::configuration::{
    BranchScheme, BuilderResolution, CommitAuthor, Component, ComponentCheckout, Forge, ForgeOwner,
    SynchronizerConfig,
};
use synchronizer::driver::{RunBoundaries, SynchronizerRun};
use synchronizer::report::{Action, FailureStage, PinValue, Verification, VerificationGate};
use synchronizer::topology::PinLayer;
use synchronizer::types::{
    AbsolutePath, AuthorEmail, AuthorName, BranchName, BuilderHost, ComponentName,
};

fn frame_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"signal-frame\"\n",
                "version = \"0.3.0\"\n",
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
                "name = \"signal-frame\"\n",
                "version = \"0.3.0\"\n",
            )
            .to_string(),
        ),
    ])
}

fn router_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"signal-router\"\n",
                "version = \"0.1.0\"\n",
                "\n",
                "[dependencies]\n",
                "signal-frame = { git = \"https://github.com/LiGoldragon/signal-frame.git\", branch = \"main\" }\n",
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
                    "name = \"signal-frame\"\n",
                    "version = \"0.2.0\"\n",
                    "source = \"git+https://github.com/LiGoldragon/signal-frame.git?branch=main#{frame_old}\"\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"signal-router\"\n",
                    "version = \"0.1.0\"\n",
                    "dependencies = [\n \"signal-frame\",\n]\n",
                ),
                frame_old = revision("frame-old").as_str()
            ),
        ),
        (
            "flake.nix".to_string(),
            concat!(
                "{\n",
                "  inputs = {\n",
                "    signal-frame.url = \"github:LiGoldragon/signal-frame\";\n",
                "  };\n",
                "  outputs = { ... }: { };\n",
                "}\n",
            )
            .to_string(),
        ),
        (
            "flake.lock".to_string(),
            format!(
                concat!(
                    "{{\n",
                    "  \"nodes\": {{\n",
                    "    \"root\": {{\n",
                    "      \"inputs\": {{\n",
                    "        \"signal-frame\": \"signal-frame\"\n",
                    "      }}\n",
                    "    }},\n",
                    "    \"signal-frame\": {{\n",
                    "      \"locked\": {{\n",
                    "        \"lastModified\": 1750000000,\n",
                    "        \"narHash\": \"sha256-old\",\n",
                    "        \"owner\": \"LiGoldragon\",\n",
                    "        \"repo\": \"signal-frame\",\n",
                    "        \"rev\": \"{frame_old}\",\n",
                    "        \"type\": \"github\"\n",
                    "      }},\n",
                    "      \"original\": {{\n",
                    "        \"owner\": \"LiGoldragon\",\n",
                    "        \"repo\": \"signal-frame\",\n",
                    "        \"type\": \"github\"\n",
                    "      }}\n",
                    "    }}\n",
                    "  }},\n",
                    "  \"root\": \"root\",\n",
                    "  \"version\": 7\n",
                    "}}\n",
                ),
                frame_old = revision("frame-old").as_str()
            ),
        ),
    ])
}

fn harness_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"signal-harness\"\n",
                "version = \"0.1.0\"\n",
                "\n",
                "[dependencies]\n",
                "# The router contract this harness supervises. Load-bearing comment:\n",
                "# it must survive a synchronizer bump byte-for-byte.\n",
                "signal-router = { git = \"https://github.com/LiGoldragon/signal-router.git\", branch = \"main\" }\n",
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
                    "name = \"signal-frame\"\n",
                    "version = \"0.2.0\"\n",
                    "source = \"git+https://github.com/LiGoldragon/signal-frame.git?branch=main#{frame_old}\"\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"signal-harness\"\n",
                    "version = \"0.1.0\"\n",
                    "dependencies = [\n \"signal-router\",\n]\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"signal-router\"\n",
                    "version = \"0.1.0\"\n",
                    "source = \"git+https://github.com/LiGoldragon/signal-router.git?branch=main#{router_old}\"\n",
                    "dependencies = [\n \"signal-frame\",\n]\n",
                ),
                frame_old = revision("frame-old").as_str(),
                router_old = revision("router-old").as_str()
            ),
        ),
        (
            "flake.nix".to_string(),
            concat!(
                "{\n",
                "  inputs = {\n",
                "    signal-router.url = \"github:LiGoldragon/signal-router/main\";\n",
                "  };\n",
                "  outputs = { ... }: { };\n",
                "}\n",
            )
            .to_string(),
        ),
        (
            "flake.lock".to_string(),
            format!(
                concat!(
                    "{{\n",
                    "  \"nodes\": {{\n",
                    "    \"root\": {{\n",
                    "      \"inputs\": {{\n",
                    "        \"signal-router\": \"signal-router\"\n",
                    "      }}\n",
                    "    }},\n",
                    "    \"signal-router\": {{\n",
                    "      \"locked\": {{\n",
                    "        \"lastModified\": 1750000000,\n",
                    "        \"narHash\": \"sha256-old\",\n",
                    "        \"owner\": \"LiGoldragon\",\n",
                    "        \"repo\": \"signal-router\",\n",
                    "        \"rev\": \"{router_old}\",\n",
                    "        \"type\": \"github\"\n",
                    "      }},\n",
                    "      \"original\": {{\n",
                    "        \"owner\": \"LiGoldragon\",\n",
                    "        \"ref\": \"main\",\n",
                    "        \"repo\": \"signal-router\",\n",
                    "        \"type\": \"github\"\n",
                    "      }}\n",
                    "    }}\n",
                    "  }},\n",
                    "  \"root\": \"root\",\n",
                    "  \"version\": 7\n",
                    "}}\n",
                ),
                router_old = revision("router-old").as_str()
            ),
        ),
    ])
}

#[test]
fn the_ascent_cascades_bumps_from_the_leaves() {
    let frame = Rc::new(FixtureRepository::new(
        "signal-frame",
        revision("frame-new"),
        frame_files(),
    ));
    let router = Rc::new(FixtureRepository::new(
        "signal-router",
        revision("router-main"),
        router_files(),
    ));
    let harness = Rc::new(FixtureRepository::new(
        "signal-harness",
        revision("harness-main"),
        harness_files(),
    ));
    let opener = FixtureOpener {
        repositories: BTreeMap::from([
            (ComponentName::new("signal-frame"), Rc::clone(&frame)),
            (ComponentName::new("signal-router"), Rc::clone(&router)),
            (ComponentName::new("signal-harness"), Rc::clone(&harness)),
        ]),
    };
    let verified = Rc::new(RefCell::new(Vec::new()));
    let config = standard_config(vec![
        Component::new(
            ComponentName::new("signal-frame"),
            ComponentCheckout::AtRoot,
        ),
        Component::new(
            ComponentName::new("signal-router"),
            ComponentCheckout::AtRoot,
        ),
        Component::new(
            ComponentName::new("signal-harness"),
            ComponentCheckout::AtRoot,
        ),
    ]);
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
                witness: "ascent witness",
            }),
        },
    );
    let report = run.execute().expect("the fixture ascent completes");
    assert!(
        !report.has_failures(),
        "collected failures: {:?}",
        report.failures()
    );

    // Level 0: the leaf is already aligned.
    let levels = report.levels();
    assert_eq!(levels.len(), 3);
    let frame_outcome = &levels[0].repositories()[0];
    assert_eq!(
        frame_outcome.component(),
        &ComponentName::new("signal-frame")
    );
    assert_eq!(frame_outcome.action(), &Action::AlreadyAligned);
    assert_eq!(frame_outcome.verification(), &Verification::NotAttempted);

    // Level 1: the router bumps its frame pins toward the frame *main*
    // tip — lock layers only, no manifest redirect for a main-tip target.
    let router_outcome = &levels[1].repositories()[0];
    let Action::Bumped(router_bump) = router_outcome.action() else {
        panic!("router must bump: {router_outcome:?}");
    };
    let router_layers: Vec<PinLayer> = router_bump
        .applied()
        .iter()
        .map(|bump| bump.layer())
        .collect();
    assert_eq!(
        router_layers,
        vec![PinLayer::CargoLock, PinLayer::FlakeLock]
    );
    for bump in router_bump.applied() {
        assert_eq!(bump.dependency(), &ComponentName::new("signal-frame"));
        assert_eq!(bump.previous(), &PinValue::Revision(revision("frame-old")));
        assert_eq!(bump.next(), &PinValue::Revision(revision("frame-new")));
    }
    let router_tip = router_bump.pushed().tip().clone();
    assert_eq!(
        router.pushed.borrow().as_slice(),
        std::slice::from_ref(&router_tip)
    );
    assert_eq!(
        router_outcome.verification(),
        &Verification::Verified(BuilderHost::new("prometheus"), VerificationGate::WireChecks)
    );

    // The router's committed lock synchronizes the recorded version to the
    // frame's manifest version at the target revision.
    let router_lock = router
        .file_text(&router_tip, "Cargo.lock")
        .expect("the bump commit carries the lock");
    assert!(router_lock.contains("version = \"0.3.0\""));
    assert!(router_lock.contains(&format!("?branch=main#{}", revision("frame-new").as_str())));

    // Level 2: the harness pins the router's pushed *synchronizer* tip —
    // the ledger rule — and its transitive frame entry moves to the frame
    // main tip.
    let harness_outcome = &levels[2].repositories()[0];
    let Action::Bumped(harness_bump) = harness_outcome.action() else {
        panic!("harness must bump: {harness_outcome:?}");
    };
    let router_name = ComponentName::new("signal-router");
    let manifest_bump = harness_bump
        .applied()
        .iter()
        .find(|bump| bump.layer() == PinLayer::CargoManifest)
        .expect("a cascade redirects the manifest declaration");
    assert_eq!(manifest_bump.dependency(), &router_name);
    assert_eq!(
        manifest_bump.previous(),
        &PinValue::Reference(BranchName::new("main"))
    );
    assert_eq!(
        manifest_bump.next(),
        &PinValue::Reference(BranchName::new("synchronizer"))
    );
    let router_lock_bump = harness_bump
        .applied()
        .iter()
        .find(|bump| bump.layer() == PinLayer::CargoLock && bump.dependency() == &router_name)
        .expect("the router lock pin moves");
    assert_eq!(
        router_lock_bump.next(),
        &PinValue::Revision(router_tip.clone()),
        "the harness pins the tip the router pushed this run, not the router main"
    );
    let frame_lock_bump = harness_bump
        .applied()
        .iter()
        .find(|bump| {
            bump.layer() == PinLayer::CargoLock
                && bump.dependency() == &ComponentName::new("signal-frame")
        })
        .expect("the transitive frame entry moves too");
    assert_eq!(
        frame_lock_bump.next(),
        &PinValue::Revision(revision("frame-new"))
    );
    let flake_bump = harness_bump
        .applied()
        .iter()
        .find(|bump| bump.layer() == PinLayer::FlakeLock)
        .expect("the flake lock pin moves");
    assert_eq!(flake_bump.next(), &PinValue::Revision(router_tip.clone()));

    // The committed harness manifest keeps its load-bearing comment and
    // redirects the branch.
    let harness_tip = harness_bump.pushed().tip().clone();
    let harness_manifest = harness
        .file_text(&harness_tip, "Cargo.toml")
        .expect("the bump commit carries the manifest");
    assert!(
        harness_manifest.contains("# it must survive a synchronizer bump byte-for-byte."),
        "comments survive the format-preserving edit"
    );
    assert!(harness_manifest.contains("branch = \"synchronizer\""));

    // The committed harness flake.lock carries the cascade in the locked
    // rev alone; the original keeps asking for what flake.nix declares.
    // Nix re-resolves originals from flake.nix on update, so a redirected
    // original would be discarded and the input re-locked to main.
    let harness_flake_lock = harness
        .file_text(&harness_tip, "flake.lock")
        .expect("the bump commit carries the flake lock");
    assert!(
        harness_flake_lock.contains("\"ref\": \"main\""),
        "the original is preserved as flake.nix declares it"
    );
    assert!(
        !harness_flake_lock.contains("\"ref\": \"synchronizer\""),
        "no original is redirected to the synchronizer branch"
    );
    assert!(harness_flake_lock.contains(&format!("\"rev\": \"{}\"", router_tip.as_str())));

    // The frame was never committed to or pushed; only synchronizer
    // branches of bumped repos were pushed.
    assert!(frame.pushed.borrow().is_empty());
    assert_eq!(harness.pushed.borrow().len(), 1);

    // Every pushed bump was verified on the role-resolved host, in ascent
    // order.
    assert_eq!(
        verified.borrow().as_slice(),
        &[
            (ComponentName::new("signal-router"), router_tip),
            (ComponentName::new("signal-harness"), harness_tip),
        ]
    );
}

/// §9 collect-and-continue: a producer whose fetch fails is not a
/// dependency cycle. Its consumers are placed in the ascent, their
/// resolution failures are collected, and the run completes with a report
/// instead of dying on `Error::DependencyCycle`.
#[test]
fn a_fetch_failed_producer_collects_failures_and_the_ascent_continues() {
    // The opener knows only the router: signal-frame's load fails at the
    // fetch stage, yet the router still pins it in its manifests.
    let router = Rc::new(FixtureRepository::new(
        "signal-router",
        revision("router-main"),
        router_files(),
    ));
    let opener = FixtureOpener {
        repositories: BTreeMap::from([(ComponentName::new("signal-router"), Rc::clone(&router))]),
    };
    let verified = Rc::new(RefCell::new(Vec::new()));
    let config = standard_config(vec![
        Component::new(
            ComponentName::new("signal-frame"),
            ComponentCheckout::AtRoot,
        ),
        Component::new(
            ComponentName::new("signal-router"),
            ComponentCheckout::AtRoot,
        ),
    ]);
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
                witness: "fetch-failure witness",
            }),
        },
    );
    let report = run
        .execute()
        .expect("an unloaded producer is a collected failure, never run-fatal");
    let stages: Vec<FailureStage> = report
        .failures()
        .iter()
        .map(|failure| failure.stage())
        .collect();
    assert!(
        stages.contains(&FailureStage::Fetch),
        "the frame's failed load is collected: {stages:?}"
    );
    assert!(
        stages.contains(&FailureStage::Resolve),
        "the router's unresolvable frame pin is collected: {stages:?}"
    );
    let router_outcome = report
        .levels()
        .iter()
        .flat_map(|level| level.repositories())
        .find(|outcome| outcome.component() == &ComponentName::new("signal-router"))
        .expect("the router joins the ascent despite the unloaded producer");
    assert_eq!(
        router_outcome.action(),
        &Action::BumpFailed(FailureStage::Resolve)
    );
    assert!(
        router.pushed.borrow().is_empty(),
        "nothing is pushed for a consumer whose resolution failed"
    );
}

/// Universality witness: a project with no criome fact whatsoever cascades
/// end to end through the generic paths. A different forge account
/// (`octocat`), a `trunk` mainline, a `bump-train` staging branch, a directly
/// named builder host, and the default-build verify policy — nothing assumes
/// `main` or `synchronizer`. The cascade redirects gamma's manifest to the
/// configured `bump-train` branch, proving no branch name is hard-coded in
/// the ascent.
mod generic {
    use super::*;

    fn generic_config() -> SynchronizerConfig {
        SynchronizerConfig::new(
            Forge::GitHub(ForgeOwner::new("octocat")),
            AbsolutePath::new("/home/dev/src"),
            vec![
                Component::new(ComponentName::new("alpha"), ComponentCheckout::AtRoot),
                Component::new(ComponentName::new("beta"), ComponentCheckout::AtRoot),
                Component::new(ComponentName::new("gamma"), ComponentCheckout::AtRoot),
            ],
            BranchScheme::new(BranchName::new("trunk"), BranchName::new("bump-train")),
            BuilderResolution::DirectHost(BuilderHost::new("buildbox.local")),
            VerifyPolicy::DefaultBuild,
            CommitAuthor::new(
                AuthorName::new("ci-bot"),
                AuthorEmail::new("ci@octocat.example"),
            ),
        )
    }

    fn alpha_files() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "Cargo.toml".to_string(),
                concat!(
                    "[package]\n",
                    "name = \"alpha\"\n",
                    "version = \"0.3.0\"\n",
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
                    "name = \"alpha\"\n",
                    "version = \"0.3.0\"\n",
                )
                .to_string(),
            ),
        ])
    }

    fn beta_files() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "Cargo.toml".to_string(),
                concat!(
                    "[package]\n",
                    "name = \"beta\"\n",
                    "version = \"0.1.0\"\n",
                    "\n",
                    "[dependencies]\n",
                    "alpha = { git = \"https://github.com/octocat/alpha.git\", branch = \"trunk\" }\n",
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
                        "name = \"alpha\"\n",
                        "version = \"0.2.0\"\n",
                        "source = \"git+https://github.com/octocat/alpha.git?branch=trunk#{alpha_old}\"\n",
                        "\n",
                        "[[package]]\n",
                        "name = \"beta\"\n",
                        "version = \"0.1.0\"\n",
                        "dependencies = [\n \"alpha\",\n]\n",
                    ),
                    alpha_old = revision("alpha-old").as_str()
                ),
            ),
        ])
    }

    fn gamma_files() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "Cargo.toml".to_string(),
                concat!(
                    "[package]\n",
                    "name = \"gamma\"\n",
                    "version = \"0.1.0\"\n",
                    "\n",
                    "[dependencies]\n",
                    "beta = { git = \"https://github.com/octocat/beta.git\", branch = \"trunk\" }\n",
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
                        "name = \"alpha\"\n",
                        "version = \"0.2.0\"\n",
                        "source = \"git+https://github.com/octocat/alpha.git?branch=trunk#{alpha_old}\"\n",
                        "\n",
                        "[[package]]\n",
                        "name = \"beta\"\n",
                        "version = \"0.1.0\"\n",
                        "source = \"git+https://github.com/octocat/beta.git?branch=trunk#{beta_old}\"\n",
                        "dependencies = [\n \"alpha\",\n]\n",
                        "\n",
                        "[[package]]\n",
                        "name = \"gamma\"\n",
                        "version = \"0.1.0\"\n",
                        "dependencies = [\n \"beta\",\n]\n",
                    ),
                    alpha_old = revision("alpha-old").as_str(),
                    beta_old = revision("beta-old").as_str()
                ),
            ),
        ])
    }

    #[test]
    fn a_non_criome_project_cascades_through_generic_config() {
        let alpha = Rc::new(FixtureRepository::new(
            "alpha",
            revision("alpha-new"),
            alpha_files(),
        ));
        let beta = Rc::new(FixtureRepository::new(
            "beta",
            revision("beta-trunk"),
            beta_files(),
        ));
        let gamma = Rc::new(FixtureRepository::new(
            "gamma",
            revision("gamma-trunk"),
            gamma_files(),
        ));
        let opener = FixtureOpener {
            repositories: BTreeMap::from([
                (ComponentName::new("alpha"), Rc::clone(&alpha)),
                (ComponentName::new("beta"), Rc::clone(&beta)),
                (ComponentName::new("gamma"), Rc::clone(&gamma)),
            ]),
        };
        let verified = Rc::new(RefCell::new(Vec::new()));
        let run = SynchronizerRun::with_boundaries(
            generic_config(),
            RunBoundaries {
                repository_opener: Box::new(opener),
                nar_hash_source: Box::new(FixturePrefetch),
                builder_host_resolver: Box::new(FixtureBuilderHost {
                    host: BuilderHost::new("buildbox.local"),
                }),
                verifier_source: Box::new(FixtureVerifierSource {
                    verified: Rc::clone(&verified),
                }),
                lock_resolver: Box::new(UnreachableLockResolver {
                    witness: "generic ascent witness",
                }),
            },
        );
        let report = run.execute().expect("the generic ascent completes");
        assert!(
            !report.has_failures(),
            "collected failures: {:?}",
            report.failures()
        );

        // beta bumps its alpha lock toward alpha's trunk tip (a main-tip
        // target: no manifest redirect, the trunk declaration already
        // reaches it).
        let levels = report.levels();
        let beta_outcome = &levels[1].repositories()[0];
        let Action::Bumped(beta_bump) = beta_outcome.action() else {
            panic!("beta must bump: {beta_outcome:?}");
        };
        let beta_layers: Vec<PinLayer> = beta_bump
            .applied()
            .iter()
            .map(|bump| bump.layer())
            .collect();
        assert_eq!(
            beta_layers,
            vec![PinLayer::CargoLock],
            "a trunk-declared main-tip target moves the lock only"
        );
        let beta_tip = beta_bump.pushed().tip().clone();
        assert_eq!(
            beta_bump.pushed().branch(),
            &BranchName::new("bump-train"),
            "the tool pushes the configured staging branch, not a hard-coded one"
        );

        // gamma pins beta's pushed staging tip and redirects its manifest to
        // the configured staging branch — never `synchronizer`.
        let gamma_outcome = &levels[2].repositories()[0];
        let Action::Bumped(gamma_bump) = gamma_outcome.action() else {
            panic!("gamma must bump: {gamma_outcome:?}");
        };
        let manifest_bump = gamma_bump
            .applied()
            .iter()
            .find(|bump| bump.layer() == PinLayer::CargoManifest)
            .expect("the cascade redirects gamma's manifest declaration");
        assert_eq!(manifest_bump.dependency(), &ComponentName::new("beta"));
        assert_eq!(
            manifest_bump.previous(),
            &PinValue::Reference(BranchName::new("trunk")),
            "the previous declaration is the configured mainline, not `main`"
        );
        assert_eq!(
            manifest_bump.next(),
            &PinValue::Reference(BranchName::new("bump-train")),
            "the redirect targets the configured staging branch, not `synchronizer`"
        );
        let beta_lock_bump = gamma_bump
            .applied()
            .iter()
            .find(|bump| {
                bump.layer() == PinLayer::CargoLock
                    && bump.dependency() == &ComponentName::new("beta")
            })
            .expect("gamma's beta lock pin moves");
        assert_eq!(
            beta_lock_bump.next(),
            &PinValue::Revision(beta_tip.clone()),
            "gamma pins the staging tip beta pushed this run"
        );

        // The committed gamma manifest carries the configured staging branch.
        let gamma_tip = gamma_bump.pushed().tip().clone();
        let gamma_manifest = gamma
            .file_text(&gamma_tip, "Cargo.toml")
            .expect("the bump commit carries the manifest");
        assert!(gamma_manifest.contains("branch = \"bump-train\""));
        assert!(!gamma_manifest.contains("synchronizer"));

        assert!(alpha.pushed.borrow().is_empty());
        assert_eq!(beta.pushed.borrow().len(), 1);
        assert_eq!(gamma.pushed.borrow().len(), 1);
    }
}
