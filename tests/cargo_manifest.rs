//! Witnesses for the format-preserving Cargo.toml edit surface.
//!
//! Pin writes are psyche-locked to be non-destructive: comments and layout
//! survive a cascade redirect byte-for-byte, with only the edited value
//! changing.

use synchronizer::cargo_manifest::{CargoManifest, DependencyName, GitReference, GitSource};
use synchronizer::error::{Error, UnbumpablePinReason};
use synchronizer::types::{BranchName, ComponentName};

/// A manifest in the workspace style: aligned `=`, load-bearing comments on
/// features and dependencies (spirit's manifest is the reference case).
const COMMENTED_MANIFEST: &str = r#"[package]
name         = "spirit"
version      = "0.21.0"
edition      = "2024"
publish      = false

[dependencies]
# The typed NOTA reader — the canonical codec every schema decode goes
# through. Renamed package: the repository is nota-next.
nota       = { package = "nota", git = "https://github.com/LiGoldragon/nota-next.git", branch = "main" }
serde      = { version = "1", features = ["derive"] }
# The wire frame carrying every socket payload. Version skew here is the
# rkyv decode failure class the synchronizer exists to kill.
signal-frame = { git = "https://github.com/LiGoldragon/signal-frame.git", branch = "main" }

[features]
# The gated mirror shipper: OFF by default and deploy-gated.
mirror-shipper = ["dep:mirror"]

[lints.rust]
unsafe_code = "forbid"
"#;

#[test]
fn cascade_redirect_preserves_comments_and_layout() {
    let component = ComponentName::new("spirit");
    let mut manifest =
        CargoManifest::from_toml_text(COMMENTED_MANIFEST, &component).expect("manifest decodes");
    let previous = manifest
        .redirect_git_dependency(
            &DependencyName::new("signal-frame"),
            GitReference::Branch(BranchName::synchronizer()),
        )
        .expect("declared git dependency redirects");
    assert_eq!(previous, GitReference::Branch(BranchName::main()));

    let rendered = manifest.to_toml_text();
    let expected = COMMENTED_MANIFEST.replace(
        "signal-frame = { git = \"https://github.com/LiGoldragon/signal-frame.git\", branch = \"main\" }",
        "signal-frame = { git = \"https://github.com/LiGoldragon/signal-frame.git\", branch = \"synchronizer\" }",
    );
    assert_eq!(
        rendered.as_str(),
        expected,
        "only the redirected branch value changes; every comment and byte of layout survives"
    );
}

/// A manifest whose dependency key differs from the resolved package name
/// (`nota-next = { package = "nota", ... }` — the real corpus shape), one
/// deliberately rev-pinned entry, and one package aliased by two entries.
const RENAMED_AND_PINNED_MANIFEST: &str = r#"[package]
name    = "consumer"
version = "0.1.0"

[dependencies]
# Renamed: the table key is the repository-flavored name, the package the
# crate name. The document must be addressed by the key.
nota-next   = { package = "nota", git = "https://github.com/LiGoldragon/nota-next.git", branch = "main" }
# Deliberately pinned at an old revision (the sema-engine case).
sema-engine = { git = "https://github.com/LiGoldragon/sema-engine.git", rev = "3333333333333333333333333333333333333333" }
signal-frame = { git = "https://github.com/LiGoldragon/signal-frame.git", branch = "main" }

[dev-dependencies]
# The same package pinned again under another key: aliased.
signal-frame-dev = { package = "signal-frame", git = "https://github.com/LiGoldragon/signal-frame.git", branch = "main" }
"#;

/// The cascade redirect addresses the document by the entry's table key,
/// not the resolved package name: `nota` lives under the `nota-next` key.
#[test]
fn cascade_redirect_addresses_renamed_dependencies_by_table_key() {
    let component = ComponentName::new("consumer");
    let mut manifest = CargoManifest::from_toml_text(RENAMED_AND_PINNED_MANIFEST, &component)
        .expect("manifest decodes");
    let previous = manifest
        .redirect_git_dependency(
            &DependencyName::new("nota"),
            GitReference::Branch(BranchName::synchronizer()),
        )
        .expect("a renamed dependency redirects through its table key");
    assert_eq!(previous, GitReference::Branch(BranchName::main()));
    let rendered = manifest.to_toml_text();
    let expected = RENAMED_AND_PINNED_MANIFEST.replace(
        "nota-next   = { package = \"nota\", git = \"https://github.com/LiGoldragon/nota-next.git\", branch = \"main\" }",
        "nota-next   = { package = \"nota\", git = \"https://github.com/LiGoldragon/nota-next.git\", branch = \"synchronizer\" }",
    );
    assert_eq!(
        rendered.as_str(),
        expected,
        "only the renamed entry's branch changes; comments and layout survive"
    );
}

/// A deliberately rev-pinned dependency fails loud: inserting `branch`
/// beside `rev` would emit an invalid manifest, and the pin is a choice
/// the tool must not override.
#[test]
fn deliberate_revision_pins_fail_loud_instead_of_emitting_invalid_manifests() {
    let component = ComponentName::new("consumer");
    let mut manifest = CargoManifest::from_toml_text(RENAMED_AND_PINNED_MANIFEST, &component)
        .expect("manifest decodes");
    let before = manifest.to_toml_text();
    let error = manifest
        .redirect_git_dependency(
            &DependencyName::new("sema-engine"),
            GitReference::Branch(BranchName::synchronizer()),
        )
        .expect_err("a rev-pinned dependency must not be redirected");
    assert!(
        matches!(
            &error,
            Error::UnbumpablePin {
                reason: UnbumpablePinReason::DeliberateRevisionPin,
                ..
            }
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        manifest.to_toml_text().as_str(),
        before.as_str(),
        "the manifest is left untouched"
    );
}

/// A package pinned by several same-name entries fails loud: addressing
/// by name would silently alias the first match.
#[test]
fn a_package_aliased_by_several_entries_fails_loud() {
    let component = ComponentName::new("consumer");
    let mut manifest = CargoManifest::from_toml_text(RENAMED_AND_PINNED_MANIFEST, &component)
        .expect("manifest decodes");
    let error = manifest
        .redirect_git_dependency(
            &DependencyName::new("signal-frame"),
            GitReference::Branch(BranchName::synchronizer()),
        )
        .expect_err("an aliased package must not be redirected by first match");
    assert!(
        matches!(
            &error,
            Error::UnbumpablePin {
                reason: UnbumpablePinReason::MultipleEntries,
                ..
            }
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn reading_finds_git_dependencies_with_package_renames() {
    let component = ComponentName::new("spirit");
    let manifest =
        CargoManifest::from_toml_text(COMMENTED_MANIFEST, &component).expect("manifest decodes");
    let dependencies: Vec<(DependencyName, GitSource)> = manifest.git_dependencies();
    let names: Vec<&str> = dependencies.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, vec!["nota", "signal-frame"]);
    let (_, nota_source) = &dependencies[0];
    assert_eq!(
        nota_source.url().as_str(),
        "https://github.com/LiGoldragon/nota-next.git"
    );
    assert_eq!(
        manifest.package_version().map(|version| version.as_str()),
        Some("0.21.0")
    );
}
