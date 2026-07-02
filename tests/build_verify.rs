//! Witnesses for the wire-exercising verify selection.
//!
//! The psyche-locked gate: where a repository exposes flake checks that
//! build and launch the daemons, those checks are the verify; only where no
//! such check exists does the verify fall back to the default `nix build`.

use synchronizer::build_verify::{CheckName, VerificationTarget, WireCheckClassifier};
use synchronizer::types::FlakeReference;

fn names(raw: &[&str]) -> Vec<CheckName> {
    raw.iter().map(|name| CheckName::new(*name)).collect()
}

#[test]
fn daemon_launching_checks_are_selected_as_the_gate() {
    let classifier = WireCheckClassifier::workspace();
    // The harness repository's real check-name shapes.
    let target = classifier.select(&names(&[
        "build",
        "clippy",
        "default",
        "fmt",
        "harness-cli-reaches-working-socket",
        "harness-daemon-answers-status-readiness",
        "harness-daemon-delivers-message-to-terminal-endpoint",
        "harness-identity-projection-views",
        "test",
    ]));
    let VerificationTarget::WireChecks(checks) = target else {
        panic!("a repository with daemon checks must not fall back to the default build");
    };
    let selected: Vec<&str> = checks.iter().map(|check| check.as_str()).collect();
    assert_eq!(
        selected,
        vec![
            "harness-cli-reaches-working-socket",
            "harness-daemon-answers-status-readiness",
            "harness-daemon-delivers-message-to-terminal-endpoint",
        ],
        "style, plain-build, and non-wire witnesses stay out of the gate"
    );
}

#[test]
fn repositories_without_wire_checks_fall_back_to_the_default_build() {
    let classifier = WireCheckClassifier::workspace();
    let target = classifier.select(&names(&["build", "clippy", "fmt", "test", "test-basic"]));
    assert_eq!(target, VerificationTarget::DefaultPackage);
    let reference = FlakeReference::new(
        "github:LiGoldragon/signal-frame/1111111111111111111111111111111111111111",
    );
    assert_eq!(
        target.installables(&reference, "x86_64-linux"),
        vec![reference.as_str().to_string()]
    );
}

#[test]
fn wire_checks_address_the_pushed_revision_per_system() {
    let target = VerificationTarget::WireChecks(names(&["test-daemon-socket"]));
    let reference = FlakeReference::new(
        "github:LiGoldragon/introspect/2222222222222222222222222222222222222222",
    );
    assert_eq!(
        target.installables(&reference, "x86_64-linux"),
        vec![
            "github:LiGoldragon/introspect/2222222222222222222222222222222222222222#checks.x86_64-linux.test-daemon-socket"
                .to_string()
        ]
    );
}
