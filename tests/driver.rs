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
    FixtureOpener, FixturePrefetch, FixtureRepository, FixtureRoleDirectory, FixtureVerifierSource,
    UnreachableLockResolver, revision,
};
use synchronizer::configuration::{
    ClusterConfiguration, Component, ComponentCheckout, Forge, ForgeOwner, SynchronizerConfig,
};
use synchronizer::driver::{RunBoundaries, SynchronizerRun};
use synchronizer::report::{Action, PinValue, Verification};
use synchronizer::topology::PinLayer;
use synchronizer::types::{AbsolutePath, BranchName, BuilderHost, BuilderRole, ComponentName};

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
    let config = SynchronizerConfig::new(
        Forge::GitHub(ForgeOwner::new("LiGoldragon")),
        AbsolutePath::new("/git/github.com/LiGoldragon"),
        vec![
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
        ],
        BuilderRole::new("NixBuilder"),
        ClusterConfiguration::ClusterProposal(AbsolutePath::new("/cluster/datom.nota")),
    );
    let run = SynchronizerRun::with_boundaries(
        config,
        RunBoundaries {
            repository_opener: Box::new(opener),
            nar_hash_source: Box::new(FixturePrefetch),
            role_directory: Box::new(FixtureRoleDirectory {
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
        &Verification::Verified(BuilderHost::new("prometheus"))
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
        &PinValue::Reference(BranchName::main())
    );
    assert_eq!(
        manifest_bump.next(),
        &PinValue::Reference(BranchName::synchronizer())
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

    // The committed harness flake.lock follows the synchronizer branch so a
    // later `nix flake update` stays on the cascade.
    let harness_flake_lock = harness
        .file_text(&harness_tip, "flake.lock")
        .expect("the bump commit carries the flake lock");
    assert!(harness_flake_lock.contains("\"ref\": \"synchronizer\""));
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
