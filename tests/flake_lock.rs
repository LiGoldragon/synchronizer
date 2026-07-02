//! Witnesses for the typed flake.lock model: byte round-trip fidelity in
//! Nix's own rendering, and the rev-set repin.

use synchronizer::flake_lock::{FlakeLock, InputName, OriginalReferenceEdit, PrefetchedSource};
use synchronizer::types::{BranchName, CommitIdentifier, ComponentName, NarHash};

/// A lock in Nix's exact rendering: two-space indent, sorted keys, a
/// `flake = false` node, a third-party node, and a component node.
const LOCK_TEXT: &str = r#"{
  "nodes": {
    "nixpkgs": {
      "locked": {
        "lastModified": 1782918843,
        "narHash": "sha256-ETYnV9U7Sr+A45dohzZdfCZKOss4qrTkO+wgNZNvEc0=",
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "e8273b29fe1390ec8d4603f2477357555291432e",
        "type": "github"
      },
      "original": {
        "owner": "NixOS",
        "ref": "nixpkgs-unstable",
        "repo": "nixpkgs",
        "type": "github"
      }
    },
    "root": {
      "inputs": {
        "nixpkgs": "nixpkgs",
        "rust-analyzer-src": "rust-analyzer-src",
        "signal-frame": "signal-frame"
      }
    },
    "rust-analyzer-src": {
      "flake": false,
      "locked": {
        "lastModified": 1782864348,
        "narHash": "sha256-NVYhLbaefeIUftPlo3kS6qr0xd8eFJRodEiaHrvFKR4=",
        "owner": "rust-lang",
        "repo": "rust-analyzer",
        "rev": "0d381ca097a8e0375a19387874d952c0a230ac4f",
        "type": "github"
      },
      "original": {
        "owner": "rust-lang",
        "ref": "nightly",
        "repo": "rust-analyzer",
        "type": "github"
      }
    },
    "signal-frame": {
      "locked": {
        "lastModified": 1750000000,
        "narHash": "sha256-oldoldoldoldoldoldoldoldoldoldoldoldold=",
        "owner": "LiGoldragon",
        "repo": "signal-frame",
        "rev": "1111111111111111111111111111111111111111",
        "type": "github"
      },
      "original": {
        "owner": "LiGoldragon",
        "ref": "main",
        "repo": "signal-frame",
        "type": "github"
      }
    }
  },
  "root": "root",
  "version": 7
}
"#;

#[test]
fn untouched_lock_reserializes_byte_identically() {
    let component = ComponentName::new("signal-router");
    let lock = FlakeLock::from_json_text(LOCK_TEXT, &component).expect("lock decodes");
    let rendered = lock.to_json_text().expect("lock encodes");
    assert_eq!(
        rendered, LOCK_TEXT,
        "Nix's rendering survives byte-for-byte"
    );
}

#[test]
fn github_inputs_expose_only_direct_root_inputs() {
    let component = ComponentName::new("signal-router");
    let lock = FlakeLock::from_json_text(LOCK_TEXT, &component).expect("lock decodes");
    let inputs = lock.github_inputs();
    let names: Vec<&str> = inputs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, vec!["nixpkgs", "rust-analyzer-src", "signal-frame"]);
    let (_, frame) = inputs
        .iter()
        .find(|(name, _)| name.as_str() == "signal-frame")
        .expect("the component input is present");
    assert_eq!(frame.owner(), Some("LiGoldragon"));
    assert_eq!(
        frame
            .revision()
            .map(|revision| revision.as_str().to_string()),
        Some("1111111111111111111111111111111111111111".to_string())
    );
}

#[test]
fn repin_moves_exactly_the_rev_set_and_preserves_every_other_byte() {
    let component = ComponentName::new("signal-router");
    let mut lock = FlakeLock::from_json_text(LOCK_TEXT, &component).expect("lock decodes");
    let previous = lock
        .repin_input(
            &component,
            &InputName::new("signal-frame"),
            CommitIdentifier::new("2222222222222222222222222222222222222222"),
            PrefetchedSource::new(
                NarHash::new("sha256-newnewnewnewnewnewnewnewnewnewnewnewnew="),
                1_760_000_000,
            ),
            OriginalReferenceEdit::FollowBranch(BranchName::synchronizer()),
        )
        .expect("the direct root input repins");
    assert_eq!(
        previous.as_str(),
        "1111111111111111111111111111111111111111"
    );
    let rendered = lock.to_json_text().expect("lock encodes");
    let expected = LOCK_TEXT
        .replace(
            "\"lastModified\": 1750000000",
            "\"lastModified\": 1760000000",
        )
        .replace(
            "sha256-oldoldoldoldoldoldoldoldoldoldoldoldold=",
            "sha256-newnewnewnewnewnewnewnewnewnewnewnewnew=",
        )
        .replace(
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
        )
        .replace(
            "        \"owner\": \"LiGoldragon\",\n        \"ref\": \"main\",",
            "        \"owner\": \"LiGoldragon\",\n        \"ref\": \"synchronizer\",",
        );
    assert_eq!(
        rendered, expected,
        "locked.rev, narHash, lastModified, and original.ref move; nixpkgs, \
         rust-analyzer-src, and all layout survive byte-for-byte"
    );
}
