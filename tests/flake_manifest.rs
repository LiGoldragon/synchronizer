//! Witnesses for the flake.nix input-URL scanner and span rewrite.

use synchronizer::cargo_manifest::GitReference;
use synchronizer::flake_lock::InputName;
use synchronizer::flake_manifest::FlakeManifest;
use synchronizer::types::{CommitIdentifier, ComponentName};

const FLAKE_TEXT: &str = r#"{
  description = "signal-router";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    signal-frame.url = "github:LiGoldragon/signal-frame/1111111111111111111111111111111111111111";
    inputs.crane.url = "github:ipetkov/crane";
  };

  outputs = { ... }: { };
}
"#;

#[test]
fn scanner_locates_assignment_form_urls() {
    let component = ComponentName::new("signal-router");
    let manifest = FlakeManifest::from_nix_text(FLAKE_TEXT, &component).expect("flake scans");
    let names: Vec<&str> = manifest
        .inputs()
        .iter()
        .map(|occurrence| occurrence.input().as_str())
        .collect();
    // The nested-attrset form (fenix) is outside the modeled grammar; the
    // `inputs.<name>.url` and `<name>.url` forms are located.
    assert_eq!(names, vec!["nixpkgs", "signal-frame", "crane"]);
    let pinned: Vec<&str> = manifest
        .pinned_inputs()
        .iter()
        .map(|occurrence| occurrence.input().as_str())
        .collect();
    assert_eq!(
        pinned,
        vec!["signal-frame"],
        "only a revision-pinned url must be rewritten here; branch refs live in the lock"
    );
}

#[test]
fn rewrite_substitutes_exactly_the_pinned_span() {
    let component = ComponentName::new("signal-router");
    let mut manifest = FlakeManifest::from_nix_text(FLAKE_TEXT, &component).expect("flake scans");
    manifest
        .rewrite_pinned_input(
            &component,
            &InputName::new("signal-frame"),
            GitReference::Revision(CommitIdentifier::new(
                "2222222222222222222222222222222222222222",
            )),
        )
        .expect("the pinned input rewrites");
    let rendered = manifest.to_nix_text();
    let expected = FLAKE_TEXT.replace(
        "github:LiGoldragon/signal-frame/1111111111111111111111111111111111111111",
        "github:LiGoldragon/signal-frame/2222222222222222222222222222222222222222",
    );
    assert_eq!(
        rendered, expected,
        "one url literal moves; every other byte of the document survives"
    );
}
