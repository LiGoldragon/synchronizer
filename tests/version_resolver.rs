//! Witnesses for the cascade resolution rule.

mod fixtures;

use std::collections::BTreeMap;

use fixtures::{FixtureRepository, revision};
use synchronizer::component_manifests::ComponentManifests;
use synchronizer::configuration::{
    ClusterConfiguration, Component, ComponentCheckout, Forge, ForgeOwner, SynchronizerConfig,
};
use synchronizer::report::PinValue;
use synchronizer::topology::{DependencyGraph, PinLayer};
use synchronizer::types::{AbsolutePath, BranchName, BuilderRole, ComponentName};
use synchronizer::version_resolver::{ResolvedTarget, VersionResolver};

#[test]
fn unbumped_dependency_resolves_to_remote_main_tip() {
    let frame = ComponentName::new("signal-frame");
    let tip = revision("frame-main");
    let resolver = VersionResolver::new(BTreeMap::from([(frame.clone(), tip.clone())]));
    assert_eq!(
        resolver.resolve(&frame).expect("known component resolves"),
        ResolvedTarget::RemoteMainTip(tip)
    );
}

#[test]
fn bumped_dependency_resolves_to_its_synchronizer_tip() {
    let frame = ComponentName::new("signal-frame");
    let main_tip = revision("frame-main");
    let synchronizer_tip = revision("frame-sync");
    let mut resolver = VersionResolver::new(BTreeMap::from([(frame.clone(), main_tip)]));
    resolver.record_bump(frame.clone(), synchronizer_tip.clone());
    assert_eq!(
        resolver.resolve(&frame).expect("known component resolves"),
        ResolvedTarget::SynchronizerTip(synchronizer_tip)
    );
}

/// A consumer pinning signal-frame at an old revision, on both the manifest
/// (branch) and lock (revision) layers.
fn consumer_manifests() -> ComponentManifests {
    let files = BTreeMap::from([
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
                    "source = \"git+https://github.com/LiGoldragon/signal-frame.git?branch=main#{rev}\"\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"signal-router\"\n",
                    "version = \"0.1.0\"\n",
                    "dependencies = [\n \"signal-frame\",\n]\n",
                ),
                rev = revision("frame-old").as_str()
            ),
        ),
    ]);
    let tip = revision("router-main");
    let repository = FixtureRepository::new("signal-router", tip.clone(), files);
    ComponentManifests::load_at(&repository, &ComponentName::new("signal-router"), tip)
        .expect("fixture manifests load")
}

fn graph_for(consumer: &ComponentManifests) -> DependencyGraph {
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
        ],
        BuilderRole::new("NixBuilder"),
        ClusterConfiguration::ClusterProposal(AbsolutePath::new("/cluster/datom.nota")),
    );
    DependencyGraph::discover(&config, std::slice::from_ref(consumer))
        .expect("fixture topology discovers")
}

#[test]
fn staleness_is_computed_per_layer_against_the_resolved_target() {
    let consumer = consumer_manifests();
    let graph = graph_for(&consumer);
    let router = ComponentName::new("signal-router");
    let frame = ComponentName::new("signal-frame");
    let edges = graph.dependencies_of(&router);

    // Plain drift: frame's main moved. The lock is stale toward the main
    // tip; the manifest declaration (branch = "main") still reaches it.
    let resolver = VersionResolver::new(BTreeMap::from([
        (frame.clone(), revision("frame-new")),
        (router.clone(), revision("router-main")),
    ]));
    let stale = resolver
        .stale_pins(&consumer, &edges)
        .expect("staleness computes");
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].edge().layer(), PinLayer::CargoLock);
    assert_eq!(
        stale[0].pinned(),
        &PinValue::Revision(revision("frame-old"))
    );
    assert_eq!(
        stale[0].target(),
        &ResolvedTarget::RemoteMainTip(revision("frame-new"))
    );

    // Cascade: frame was bumped this run. Both layers must move — the lock
    // to the synchronizer tip, the manifest declaration to the branch that
    // can reach it.
    let mut cascade_resolver = VersionResolver::new(BTreeMap::from([
        (frame.clone(), revision("frame-new")),
        (router.clone(), revision("router-main")),
    ]));
    cascade_resolver.record_bump(frame.clone(), revision("frame-sync"));
    let stale = cascade_resolver
        .stale_pins(&consumer, &edges)
        .expect("staleness computes");
    assert_eq!(stale.len(), 2);
    let manifest_pin = stale
        .iter()
        .find(|pin| pin.edge().layer() == PinLayer::CargoManifest)
        .expect("the manifest layer is stale under a cascade");
    assert_eq!(
        manifest_pin.pinned(),
        &PinValue::Reference(BranchName::main())
    );
    assert_eq!(
        manifest_pin.target(),
        &ResolvedTarget::SynchronizerTip(revision("frame-sync"))
    );
    let lock_pin = stale
        .iter()
        .find(|pin| pin.edge().layer() == PinLayer::CargoLock)
        .expect("the lock layer is stale under a cascade");
    assert_eq!(
        lock_pin.target(),
        &ResolvedTarget::SynchronizerTip(revision("frame-sync"))
    );
}
