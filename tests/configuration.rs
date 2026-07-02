//! Witnesses for the NOTA configuration schema.
//!
//! The authoritative wire shape is the Rust schema in
//! `src/configuration.rs` plus these round-trip examples; the pseudo-NOTA
//! in ARCHITECTURE.md §3 is documentation. The root record is an untagged
//! positional struct per the canonical codec.

use synchronizer::configuration::{
    ClusterConfiguration, Component, ComponentCheckout, Forge, ForgeOwner, SynchronizerConfig,
};
use synchronizer::types::{AbsolutePath, BuilderRole, ComponentName};

fn example_document() -> &'static str {
    "((GitHub LiGoldragon)\n\
     /git/github.com/LiGoldragon\n\
     [(signal-frame AtRoot)\n\
      (signal-router AtRoot)\n\
      (signal-harness (AtPath /work/checkouts/signal-harness))\n\
      (introspect AtRoot)]\n\
     NixBuilder\n\
     (ClusterProposal /git/github.com/LiGoldragon/goldragon/datom.nota))"
}

fn example_config() -> SynchronizerConfig {
    SynchronizerConfig::new(
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
                ComponentCheckout::AtPath(AbsolutePath::new("/work/checkouts/signal-harness")),
            ),
            Component::new(ComponentName::new("introspect"), ComponentCheckout::AtRoot),
        ],
        BuilderRole::new("NixBuilder"),
        ClusterConfiguration::ClusterProposal(AbsolutePath::new(
            "/git/github.com/LiGoldragon/goldragon/datom.nota",
        )),
    )
}

#[test]
fn configuration_round_trips_through_the_canonical_codec() {
    let decoded = SynchronizerConfig::from_nota_text(example_document())
        .expect("the example document decodes");
    assert_eq!(decoded, example_config());
    let encoded = decoded.to_nota_text();
    let redecoded =
        SynchronizerConfig::from_nota_text(&encoded).expect("the encoded document decodes");
    assert_eq!(redecoded, decoded);
}

#[test]
fn at_root_checkout_resolves_against_the_checkout_root() {
    let config = example_config();
    let at_root = config
        .checkout_path(&ComponentName::new("signal-frame"))
        .expect("configured component resolves");
    assert_eq!(
        at_root,
        std::path::PathBuf::from("/git/github.com/LiGoldragon/signal-frame")
    );
    let at_path = config
        .checkout_path(&ComponentName::new("signal-harness"))
        .expect("configured component resolves");
    assert_eq!(
        at_path,
        std::path::PathBuf::from("/work/checkouts/signal-harness")
    );
    assert!(
        config
            .checkout_path(&ComponentName::new("unknown"))
            .is_err()
    );
}

#[test]
fn repository_url_names_the_forge_remote() {
    let config = example_config();
    let url = config
        .repository_url(&ComponentName::new("signal-router"))
        .expect("configured component resolves");
    assert_eq!(
        url.as_str(),
        "https://github.com/LiGoldragon/signal-router.git"
    );
}
