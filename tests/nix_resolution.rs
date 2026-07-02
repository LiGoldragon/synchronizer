//! Real-Nix resolution witness for the cascaded flake lock (stateful).
//!
//! The Preserve rule this proves: a cascade repin moves `locked.rev` only
//! and leaves the node's `original` exactly what `flake.nix` declares. Nix
//! re-resolves originals from `flake.nix` on update, so an original edited
//! to follow a staging branch is discarded and the input re-locked to the
//! declared branch tip — evaluating the *old* content and reintroducing
//! the very skew the synchronizer exists to kill. With the original
//! preserved, Nix must honor the lock and fetch the pinned revision even
//! though it is not the branch tip. This probe runs *actual* Nix
//! resolution of a github-type input (the git-type mechanism was validated
//! separately) on the role-resolved builder and asserts the evaluated
//! input revision is the repinned one, not a re-lock to the branch tip.
//!
//! Stateful requirements (run explicitly; excluded from the pure gate):
//! - the cluster proposal datom, `SYNCHRONIZER_CLUSTER_PROPOSAL` or the
//!   production default `/git/github.com/LiGoldragon/goldragon/datom.nota`,
//!   from which the builder host is role-resolved (`NixBuilder`) exactly as
//!   production does;
//! - ssh reachability of the resolved host (the production verify path);
//! - network on this machine (one read-only `git ls-remote`) and on the
//!   builder (`nix flake prefetch`, the input fetch);
//! - a temporary directory on the builder, removed by the probe itself.
//!
//! Run: `cargo test --test nix_resolution -- --ignored --nocapture`
//!
//! The probe target is a tiny, stable third-party repository so no real
//! component repository is touched: the v1.0.0 tag of numtide/flake-utils
//! plays the synchronizer tip (a revision that is not the main tip).

use synchronizer::configuration::ClusterConfiguration;
use synchronizer::flake_lock::{FlakeLock, InputName, PrefetchedSource};
use synchronizer::role_resolution::{ClusterRoleDirectory, CriomosClusterDirectory};
use synchronizer::types::{
    AbsolutePath, BuilderHost, BuilderRole, CommitIdentifier, ComponentName, NarHash,
};

const PROBE_OWNER: &str = "numtide";
const PROBE_REPOSITORY: &str = "flake-utils";
const PROBE_TAG: &str = "v1.0.0";
const PROBE_INPUT: &str = "probe";

/// One ssh session to the role-resolved builder, mirroring the production
/// verify transport (`ssh <host> -- <command>`).
struct BuilderProbe {
    host: BuilderHost,
}

impl BuilderProbe {
    fn from_cluster_proposal() -> Self {
        let path = std::env::var("SYNCHRONIZER_CLUSTER_PROPOSAL")
            .unwrap_or_else(|_| "/git/github.com/LiGoldragon/goldragon/datom.nota".to_string());
        let directory = CriomosClusterDirectory::new(ClusterConfiguration::ClusterProposal(
            AbsolutePath::new(path),
        ));
        let host = directory
            .host_for(&BuilderRole::new("NixBuilder"))
            .expect("the cluster proposal resolves the builder role");
        Self { host }
    }

    fn run(&self, script: &str) -> Result<String, String> {
        let output = std::process::Command::new("ssh")
            .arg(self.host.as_str())
            .arg("--")
            .arg(script)
            .output()
            .map_err(|error| format!("ssh {}: {error}", self.host.as_str()))?;
        if !output.status.success() {
            return Err(format!(
                "builder command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// `nix flake prefetch --json` on the builder: the narHash truth for
    /// the repinned revision, obtained where the resolution will run.
    fn prefetch(&self, revision: &CommitIdentifier) -> PrefetchedSource {
        let reply = self
            .run(&format!(
                "nix flake prefetch --json 'github:{PROBE_OWNER}/{PROBE_REPOSITORY}/{}'",
                revision.as_str()
            ))
            .expect("the builder prefetches the probe revision");
        let decoded: serde_json::Value =
            serde_json::from_str(reply.trim()).expect("the prefetch reply decodes");
        let nar_hash = decoded["hash"].as_str().expect("prefetch carries a hash");
        let last_modified = decoded["locked"]["lastModified"]
            .as_u64()
            .expect("prefetch carries lastModified");
        PrefetchedSource::new(NarHash::new(nar_hash), last_modified)
    }
}

/// The probe repository's remote truth: its main tip and the stable tag
/// revision standing in for a synchronizer tip.
struct ProbeRemote {
    main_tip: CommitIdentifier,
    off_tip_revision: CommitIdentifier,
}

impl ProbeRemote {
    fn query() -> Self {
        let output = std::process::Command::new("git")
            .args([
                "ls-remote",
                &format!("https://github.com/{PROBE_OWNER}/{PROBE_REPOSITORY}"),
                "refs/heads/main",
                &format!("refs/tags/{PROBE_TAG}"),
                &format!("refs/tags/{PROBE_TAG}^{{}}"),
            ])
            .output()
            .expect("git ls-remote runs");
        assert!(output.status.success(), "git ls-remote succeeds");
        let listing = String::from_utf8_lossy(&output.stdout).to_string();
        let revision_of = |reference: &str| {
            listing.lines().find_map(|line| {
                let (revision, name) = line.split_once('\t')?;
                (name == reference).then(|| CommitIdentifier::new(revision))
            })
        };
        let main_tip = revision_of("refs/heads/main").expect("the probe remote has a main tip");
        // A peeled tag object if annotated, the tag ref itself otherwise.
        let off_tip_revision = revision_of(&format!("refs/tags/{PROBE_TAG}^{{}}"))
            .or_else(|| revision_of(&format!("refs/tags/{PROBE_TAG}")))
            .expect("the probe remote has the stable tag");
        Self {
            main_tip,
            off_tip_revision,
        }
    }
}

/// The github-type confirmation the audit asked for: a lock cascaded by
/// the production repin path (original preserved, rev moved off the branch
/// tip) resolves — through actual Nix on the role-resolved builder — to
/// the repinned revision, not a re-lock to the declared branch's tip.
#[test]
#[ignore = "stateful: role-resolved builder over ssh, network, remote temp dir"]
fn cascaded_lock_resolves_to_the_pinned_revision_on_the_builder_not_to_main() {
    let remote = ProbeRemote::query();
    assert_ne!(
        remote.off_tip_revision, remote.main_tip,
        "probe validity: the pinned revision must differ from the main tip, \
         otherwise a re-lock would be indistinguishable from lock honoring"
    );

    let probe = BuilderProbe::from_cluster_proposal();
    let prefetched = probe.prefetch(&remote.off_tip_revision);

    // The base lock pins the main tip, exactly consistent with what
    // flake.nix declares (no ref: the default branch); the production
    // repin path then moves the rev alone — the cascade shape.
    let base_lock_text = format!(
        r#"{{
  "nodes": {{
    "{PROBE_INPUT}": {{
      "locked": {{
        "lastModified": 1,
        "narHash": "sha256-0000000000000000000000000000000000000000000=",
        "owner": "{PROBE_OWNER}",
        "repo": "{PROBE_REPOSITORY}",
        "rev": "{main_tip}",
        "type": "github"
      }},
      "original": {{
        "owner": "{PROBE_OWNER}",
        "repo": "{PROBE_REPOSITORY}",
        "type": "github"
      }}
    }},
    "root": {{
      "inputs": {{
        "{PROBE_INPUT}": "{PROBE_INPUT}"
      }}
    }}
  }},
  "root": "root",
  "version": 7
}}
"#,
        main_tip = remote.main_tip.as_str()
    );
    let consumer = ComponentName::new("synchronizer-probe");
    let mut lock =
        FlakeLock::from_json_text(&base_lock_text, &consumer).expect("the probe base lock decodes");
    let previous = lock
        .repin_input(
            &consumer,
            &InputName::new(PROBE_INPUT),
            remote.off_tip_revision.clone(),
            prefetched,
        )
        .expect("the production repin path cascades the probe input");
    assert_eq!(previous, remote.main_tip);
    let cascaded_lock = lock.to_json_text().expect("the cascaded lock encodes");
    assert!(
        !cascaded_lock.contains("\"ref\""),
        "the preserved original declares no ref, exactly like flake.nix"
    );

    let flake_manifest = format!(
        "{{\n  inputs.{PROBE_INPUT}.url = \"github:{PROBE_OWNER}/{PROBE_REPOSITORY}\";\n  outputs = {{ {PROBE_INPUT}, ... }}: {{ probeRevision = {PROBE_INPUT}.rev; }};\n}}\n"
    );

    // Materialize the consumer flake on the builder, run actual Nix
    // resolution there, and read back the revision Nix evaluated.
    let script = format!(
        "set -eu\n\
         directory=$(mktemp -d /tmp/synchronizer-nix-resolution-probe.XXXXXX)\n\
         trap 'rm -rf \"$directory\"' EXIT\n\
         cat > \"$directory/flake.nix\" <<'SYNCHRONIZER_PROBE_FLAKE'\n\
         {flake_manifest}\
         SYNCHRONIZER_PROBE_FLAKE\n\
         cat > \"$directory/flake.lock\" <<'SYNCHRONIZER_PROBE_LOCK'\n\
         {cascaded_lock}\
         SYNCHRONIZER_PROBE_LOCK\n\
         nix eval --raw --no-write-lock-file \"path:$directory#probeRevision\"\n"
    );
    let resolved = probe
        .run(&script)
        .expect("the builder resolves and evaluates the cascaded consumer flake");
    assert_eq!(
        resolved.trim(),
        remote.off_tip_revision.as_str(),
        "Nix must honor the cascaded lock (original preserved) and fetch \
         the pinned revision — a re-lock back to the main tip would \
         reintroduce the wire-skew class this tool exists to kill"
    );
}
