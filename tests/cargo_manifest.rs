//! Witnesses for the format-preserving Cargo.toml edit surface.
//!
//! Pin writes are psyche-locked to be non-destructive: comments and layout
//! survive a cascade redirect byte-for-byte, with only the edited value
//! changing.

use synchronizer::cargo_manifest::{CargoManifest, DependencyName, GitReference, GitSource};
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
