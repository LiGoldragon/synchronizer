//! Witnesses for topology discovery and the ascent order.

mod fixtures;

use std::collections::BTreeMap;

use fixtures::{FixtureRepository, revision, standard_config};
use synchronizer::component_manifests::ComponentManifests;
use synchronizer::configuration::{Component, ComponentCheckout, SynchronizerConfig};
use synchronizer::error::Error;
use synchronizer::topology::{DependencyGraph, LocalPinName, PinLayer};
use synchronizer::types::ComponentName;

fn config_for(components: &[&str]) -> SynchronizerConfig {
    standard_config(
        components
            .iter()
            .map(|name| Component::new(ComponentName::new(*name), ComponentCheckout::AtRoot))
            .collect(),
    )
}

fn manifests_for(component: &str, files: BTreeMap<String, String>) -> ComponentManifests {
    let tip = revision(&format!("{component}-main"));
    let repository = FixtureRepository::new(component, tip.clone(), files);
    ComponentManifests::load_at(&repository, &ComponentName::new(component), tip)
        .expect("fixture manifests load")
}

fn cargo_component(component: &str, dependencies: &[(&str, &str, &str)]) -> ComponentManifests {
    // dependencies: (dependency key, package name, repository name)
    let manifest_dependencies: String = dependencies
        .iter()
        .map(|(key, package, repo)| {
            let rename = if key == package {
                String::new()
            } else {
                format!("package = \"{package}\", ")
            };
            format!(
                "{key} = {{ {rename}git = \"https://github.com/LiGoldragon/{repo}.git\", branch = \"main\" }}\n"
            )
        })
        .collect();
    let lock_entries: String = dependencies
        .iter()
        .map(|(_, package, repo)| {
            format!(
                "[[package]]\nname = \"{package}\"\nversion = \"0.1.0\"\nsource = \"git+https://github.com/LiGoldragon/{repo}.git?branch=main#{rev}\"\n\n",
                rev = revision(&format!("{repo}-old")).as_str()
            )
        })
        .collect();
    let files = BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            format!(
                "[package]\nname = \"{component}\"\nversion = \"0.1.0\"\n\n[dependencies]\n{manifest_dependencies}"
            ),
        ),
        (
            "Cargo.lock".to_string(),
            format!(
                "version = 4\n\n[[package]]\nname = \"{component}\"\nversion = \"0.1.0\"\n\n{lock_entries}"
            ),
        ),
    ]);
    manifests_for(component, files)
}

#[test]
fn edges_come_only_from_manifests_matched_by_repository_identity() {
    // The `nota` package lives in the `nota-next` repository: matching must
    // follow the git URL, never the package name.
    let files = BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"consumer\"\n",
                "version = \"0.1.0\"\n",
                "\n",
                "[dependencies]\n",
                "nota = { package = \"nota\", git = \"https://github.com/LiGoldragon/nota-next.git\", branch = \"main\" }\n",
                "serde = \"1\"\n",
                "outside = { git = \"https://github.com/SomeoneElse/outside.git\", branch = \"main\" }\n",
                "impostor = { git = \"https://github.com/SomeoneElse/nota-next.git\", branch = \"main\" }\n",
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
                    "name = \"consumer\"\n",
                    "version = \"0.1.0\"\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"nota\"\n",
                    "version = \"0.5.1\"\n",
                    "source = \"git+https://github.com/LiGoldragon/nota-next.git?branch=main#{rev}\"\n",
                ),
                rev = revision("nota-old").as_str()
            ),
        ),
    ]);
    let consumer = manifests_for("consumer", files);
    let nota_next = cargo_component("nota-next", &[]);
    let config = config_for(&["consumer", "nota-next"]);
    let graph = DependencyGraph::discover(&config, &[consumer, nota_next])
        .expect("fixture topology discovers");
    let consumer_name = ComponentName::new("consumer");
    let edges = graph.dependencies_of(&consumer_name);
    assert_eq!(edges.len(), 2, "one manifest edge and one lock edge");
    for edge in &edges {
        assert_eq!(edge.producer(), &ComponentName::new("nota-next"));
        assert!(matches!(
            edge.local_name(),
            LocalPinName::CargoPackage(name) if name.as_str() == "nota"
        ));
    }
    assert!(
        edges
            .iter()
            .any(|edge| edge.layer() == PinLayer::CargoManifest)
    );
    assert!(edges.iter().any(|edge| edge.layer() == PinLayer::CargoLock));
}

/// A producer declared under several same-name manifest entries (the same
/// crate in `[dependencies]` and `[dev-dependencies]`) collapses to a single
/// manifest edge — the invariant "one edge per (consumer, producer, layer)".
/// The lock records the crate once, so the lock edge is single too.
#[test]
fn same_name_manifest_entries_collapse_to_one_edge() {
    let files = BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"router\"\n",
                "version = \"0.1.0\"\n",
                "\n",
                "[dependencies]\n",
                "signal-criome = { git = \"https://github.com/LiGoldragon/signal-criome.git\", branch = \"main\" }\n",
                "\n",
                "[dev-dependencies]\n",
                "signal-criome = { git = \"https://github.com/LiGoldragon/signal-criome.git\", branch = \"main\" }\n",
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
                    "name = \"router\"\n",
                    "version = \"0.1.0\"\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"signal-criome\"\n",
                    "version = \"0.1.0\"\n",
                    "source = \"git+https://github.com/LiGoldragon/signal-criome.git?branch=main#{rev}\"\n",
                ),
                rev = revision("signal-criome-old").as_str()
            ),
        ),
    ]);
    let router = manifests_for("router", files);
    let signal_criome = cargo_component("signal-criome", &[]);
    let config = config_for(&["router", "signal-criome"]);
    let graph =
        DependencyGraph::discover(&config, &[router, signal_criome]).expect("topology discovers");
    let edges = graph.dependencies_of(&ComponentName::new("router"));
    assert_eq!(
        edges.len(),
        2,
        "two same-name manifest entries collapse to one manifest edge, plus the single lock edge"
    );
    assert_eq!(
        edges
            .iter()
            .filter(|edge| edge.layer() == PinLayer::CargoManifest)
            .count(),
        1,
        "exactly one manifest edge for the doubly-declared producer"
    );
    assert_eq!(
        edges
            .iter()
            .filter(|edge| edge.layer() == PinLayer::CargoLock)
            .count(),
        1,
        "exactly one lock edge",
    );
}

#[test]
fn ascent_levels_put_leaves_first_and_reject_cycles() {
    let frame = cargo_component("signal-frame", &[]);
    let router = cargo_component(
        "signal-router",
        &[("signal-frame", "signal-frame", "signal-frame")],
    );
    let harness = cargo_component(
        "signal-harness",
        &[("signal-router", "signal-router", "signal-router")],
    );
    let config = config_for(&["signal-frame", "signal-router", "signal-harness"]);
    let graph = DependencyGraph::discover(&config, &[frame, router, harness])
        .expect("fixture topology discovers");
    let levels = graph.ascent_levels().expect("a chain is a DAG");
    assert_eq!(
        levels.levels(),
        &[
            vec![ComponentName::new("signal-frame")],
            vec![ComponentName::new("signal-router")],
            vec![ComponentName::new("signal-harness")],
        ]
    );

    let alpha = cargo_component("alpha", &[("beta", "beta", "beta")]);
    let beta = cargo_component("beta", &[("alpha", "alpha", "alpha")]);
    let cyclic_config = config_for(&["alpha", "beta"]);
    let cyclic = DependencyGraph::discover(&cyclic_config, &[alpha, beta])
        .expect("discovery itself accepts a cycle");
    assert!(matches!(
        cyclic.ascent_levels(),
        Err(Error::DependencyCycle { members }) if members.len() == 2
    ));
}

/// A configured producer whose manifests never loaded (its fetch failed)
/// is not a dependency cycle: its consumers are still placed in the
/// ascent, and their unresolvable edges become collected failures instead
/// of killing the run (§9 collect-and-continue).
#[test]
fn unloaded_producers_do_not_masquerade_as_cycles() {
    // signal-frame is configured but absent from the loaded manifest set:
    // its fetch failed. The router still declares the dependency.
    let router = cargo_component(
        "signal-router",
        &[("signal-frame", "signal-frame", "signal-frame")],
    );
    let config = config_for(&["signal-frame", "signal-router"]);
    let graph = DependencyGraph::discover(&config, &[router]).expect("fixture topology discovers");
    assert!(
        !graph
            .dependencies_of(&ComponentName::new("signal-router"))
            .is_empty(),
        "the edge to the unloaded producer exists"
    );
    let levels = graph
        .ascent_levels()
        .expect("an unloaded producer is not a cycle");
    assert_eq!(
        levels.levels(),
        &[vec![ComponentName::new("signal-router")]],
        "the consumer is placed; its resolution failure is collected later"
    );
}
