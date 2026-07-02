//! Witnesses for the wire-exercising verify selection.
//!
//! The psyche-locked gate: where a repository exposes flake checks that
//! build and launch the daemons, those checks are the verify; only where no
//! such check exists does the verify fall back to the default `nix build`.

use synchronizer::build_verify::{
    CheckEnumeration, CheckName, VerificationTarget, WireCheckClassifier,
};
use synchronizer::report::VerificationGate;
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

#[test]
fn the_verification_target_names_its_gate_class_for_the_report() {
    assert_eq!(
        VerificationTarget::WireChecks(names(&["test-daemon-socket"])).gate(),
        VerificationGate::WireChecks
    );
    assert_eq!(
        VerificationTarget::DefaultPackage.gate(),
        VerificationGate::DefaultPackage
    );
}

/// The check-enumeration expression treats an absent `checks` attribute as
/// data — it answers `[ ]` instead of failing — so the *only* way the
/// enumeration command fails is a genuine eval or transport failure, which
/// must never silently downgrade the verify to a plain build.
#[test]
fn check_enumeration_answers_absence_as_data_not_as_failure() {
    let reference = FlakeReference::new(
        "github:LiGoldragon/introspect/2222222222222222222222222222222222222222",
    );
    let enumeration = CheckEnumeration::new(reference, "x86_64-linux");
    let command = enumeration.command();
    assert!(
        command.contains("builtins.getFlake"),
        "the expression opens the locked flake reference: {command}"
    );
    assert!(
        command.contains("flake ? checks && flake.checks ? \"x86_64-linux\""),
        "absence is guarded in the expression itself: {command}"
    );
    assert!(
        command.contains("else [ ]"),
        "an absent checks attribute answers an empty list: {command}"
    );
}

/// An undecodable enumeration reply is a failure, never an empty check
/// list: an empty list would legitimize the default-build fallback.
#[test]
fn undecodable_check_enumeration_is_a_failure_not_an_empty_list() {
    let reference = FlakeReference::new(
        "github:LiGoldragon/introspect/2222222222222222222222222222222222222222",
    );
    let enumeration = CheckEnumeration::new(reference, "x86_64-linux");
    let decoded = enumeration
        .decode("[\"build\",\"harness-daemon-answers-status-readiness\"]\n")
        .expect("a JSON name list decodes");
    let decoded_names: Vec<&str> = decoded.iter().map(|name| name.as_str()).collect();
    assert_eq!(
        decoded_names,
        vec!["build", "harness-daemon-answers-status-readiness"]
    );
    let error = enumeration
        .decode("error: cannot connect to socket\n")
        .expect_err("garbage output is a failure, not an empty check list");
    assert!(
        error.contains("undecodable"),
        "the failure names the decode problem: {error}"
    );
}
