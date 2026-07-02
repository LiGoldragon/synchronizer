//! Witnesses for the NOTA configuration schema.
//!
//! The authoritative wire shape is the Rust schema in
//! `src/configuration.rs` plus these round-trip examples; the pseudo-NOTA
//! in ARCHITECTURE.md §3 is documentation. The root record is an untagged
//! positional struct per the canonical codec.
//!
//! Two examples witness universality: one cluster-role/wire-exercising
//! configuration, and one entirely non-criome direct-host/default-build
//! configuration. Both decode through the same generic schema — the tool
//! carries no project data.

use synchronizer::build_verify::{VerifyPolicy, WireCheckWord};
use synchronizer::configuration::{
    BranchScheme, BuilderResolution, ClusterSource, CommitAuthor, Component, ComponentCheckout,
    Forge, ForgeOwner, SynchronizerConfig,
};
use synchronizer::types::{
    AbsolutePath, AuthorEmail, AuthorName, BranchName, BuilderHost, BuilderRole, ComponentName,
};

fn words(raw: &[&str]) -> Vec<WireCheckWord> {
    raw.iter().map(|word| WireCheckWord::new(*word)).collect()
}

fn example_document() -> &'static str {
    "((GitHub LiGoldragon)\n\
     /git/github.com/LiGoldragon\n\
     [(signal-frame AtRoot)\n\
      (signal-router AtRoot)\n\
      (signal-harness (AtPath /work/checkouts/signal-harness))\n\
      (introspect AtRoot)]\n\
     (main synchronizer)\n\
     (ClusterRole (NixBuilder (ClusterProposal /git/github.com/LiGoldragon/goldragon/datom.nota)))\n\
     (WireExercising [daemon daemons socket sockets wire])\n\
     (synchronizer noreply@example.net))"
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
        BranchScheme::new(BranchName::new("main"), BranchName::new("synchronizer")),
        BuilderResolution::ClusterRole(
            BuilderRole::new("NixBuilder"),
            ClusterSource::ClusterProposal(AbsolutePath::new(
                "/git/github.com/LiGoldragon/goldragon/datom.nota",
            )),
        ),
        VerifyPolicy::WireExercising(words(&["daemon", "daemons", "socket", "sockets", "wire"])),
        CommitAuthor::new(
            AuthorName::new("synchronizer"),
            AuthorEmail::new("noreply@example.net"),
        ),
    )
}

/// A configuration that names no criome fact at all: a different forge
/// account, a `master`/`bump-train` branch scheme, a directly named builder
/// host (no cluster directory), the default-build verify policy, and a
/// distinct commit author. It exercises the generic paths end to end.
fn generic_document() -> &'static str {
    "((GitHub octocat)\n\
     /home/dev/src\n\
     [(alpha AtRoot)\n\
      (beta (AtPath /home/dev/checkouts/beta))]\n\
     (master bump-train)\n\
     (DirectHost buildbox.local)\n\
     DefaultBuild\n\
     (ci-bot ci@octocat.example))"
}

fn generic_config() -> SynchronizerConfig {
    SynchronizerConfig::new(
        Forge::GitHub(ForgeOwner::new("octocat")),
        AbsolutePath::new("/home/dev/src"),
        vec![
            Component::new(ComponentName::new("alpha"), ComponentCheckout::AtRoot),
            Component::new(
                ComponentName::new("beta"),
                ComponentCheckout::AtPath(AbsolutePath::new("/home/dev/checkouts/beta")),
            ),
        ],
        BranchScheme::new(BranchName::new("master"), BranchName::new("bump-train")),
        BuilderResolution::DirectHost(BuilderHost::new("buildbox.local")),
        VerifyPolicy::DefaultBuild,
        CommitAuthor::new(
            AuthorName::new("ci-bot"),
            AuthorEmail::new("ci@octocat.example"),
        ),
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
fn a_non_criome_configuration_decodes_through_the_generic_schema() {
    let decoded = SynchronizerConfig::from_nota_text(generic_document())
        .expect("the generic document decodes");
    assert_eq!(decoded, generic_config());
    // Round-trips too: the generic strategies are first-class wire shapes.
    let encoded = decoded.to_nota_text();
    let redecoded =
        SynchronizerConfig::from_nota_text(&encoded).expect("the encoded generic document decodes");
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
