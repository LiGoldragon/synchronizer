//! Build verification of a pushed bump.
//!
//! After a component's synchronizer branch is pushed, the pushed revision
//! is verified *remotely addressed* (`github:` flake reference) on the host
//! resolved from the configured builder role — so the verify exercises
//! exactly what a fresh consumer would fetch, and the builder sees only
//! pushed truth.
//!
//! The verify gate is psyche-locked to be **wire-exercising**: where the
//! repository's flake exposes checks that build *and launch* the daemons —
//! the class that catches build-vintage wire skew at runtime — those checks
//! are the gate. Only where no such check exists does the verify fall back
//! to the default `nix build`. A green plain build alone is not the point;
//! catching runtime wire skew is. Wide `nix flake check` sweeps remain
//! deliberately out (loop-killers).
//!
//! The absence of a `checks` attribute is *data* (an empty check list and
//! the legitimate default-build fallback); an eval or transport failure
//! while enumerating checks is a *failure*. The two are never conflated:
//! a broken ssh session or undecodable eval must not silently downgrade
//! the gate to a plain build — the one forbidden class.
//!
//! A verification failure is report data, not a crate [`Error`]: the ascent
//! continues and the failure is collected.

use std::sync::OnceLock;

use crate::configuration::Forge;
use crate::error::Error;
use crate::report::VerificationGate;
use crate::role_resolution::ClusterRoleDirectory;
use crate::types::{BuilderHost, BuilderRole, CommitIdentifier, ComponentName, FlakeReference};

/// The verify boundary the driver drives. [`BuildVerifier`] is the
/// production implementation; fixtures stand in during ascent tests.
pub trait Verifier {
    fn host(&self) -> &BuilderHost;

    /// Verify the component at `revision` and report the outcome. Never
    /// returns an error for a build failure — that is a collected outcome.
    fn verify(
        &self,
        forge: &Forge,
        component: &ComponentName,
        revision: &CommitIdentifier,
    ) -> VerificationOutcome;
}

/// One check name inside a flake's `checks.<system>` attribute set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckName(String);

impl CheckName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Selects the wire-exercising check class from a flake's check names.
///
/// The workspace convention this snapshot encodes: checks that launch
/// daemons and speak the wire carry `daemon`, `socket`, or `wire` as a
/// name word (`harness-daemon-answers-status-readiness`,
/// `test-daemon-socket`,
/// `router-generated-daemon-answers-working-and-meta-sockets`). The
/// classifier matches those words — singular or plural — against the
/// hyphen/underscore-split name. A repository joins the class by naming;
/// nothing repository-specific lives here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireCheckClassifier {
    name_words: Vec<&'static str>,
}

impl WireCheckClassifier {
    /// The workspace's wire-exercising name words.
    pub fn workspace() -> Self {
        Self {
            name_words: vec!["daemon", "daemons", "socket", "sockets", "wire"],
        }
    }

    /// Whether one check name marks a wire-exercising check.
    pub fn is_wire_exercising(&self, name: &CheckName) -> bool {
        name.as_str()
            .split(['-', '_'])
            .any(|word| self.name_words.contains(&word))
    }

    /// The verification target for a repository exposing `names`: its
    /// wire-exercising checks where it has them, the default package build
    /// otherwise.
    pub fn select(&self, names: &[CheckName]) -> VerificationTarget {
        let wire_checks: Vec<CheckName> = names
            .iter()
            .filter(|name| self.is_wire_exercising(name))
            .cloned()
            .collect();
        if wire_checks.is_empty() {
            VerificationTarget::DefaultPackage
        } else {
            VerificationTarget::WireChecks(wire_checks)
        }
    }
}

/// What a verify run builds for one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationTarget {
    /// The repository's wire-exercising checks, each addressed as
    /// `<flake>#checks.<system>.<name>`.
    WireChecks(Vec<CheckName>),
    /// No wire-exercising check exists: the default `nix build` of the
    /// flake.
    DefaultPackage,
}

impl VerificationTarget {
    /// The gate class this target represents in the run report.
    pub fn gate(&self) -> VerificationGate {
        match self {
            Self::WireChecks(_) => VerificationGate::WireChecks,
            Self::DefaultPackage => VerificationGate::DefaultPackage,
        }
    }

    /// The nix installables this target builds at `reference` on `system`.
    pub fn installables(&self, reference: &FlakeReference, system: &str) -> Vec<String> {
        match self {
            Self::WireChecks(checks) => checks
                .iter()
                .map(|check| {
                    format!(
                        "{}#checks.{}.{}",
                        reference.as_str(),
                        system,
                        check.as_str()
                    )
                })
                .collect(),
            Self::DefaultPackage => vec![reference.as_str().to_string()],
        }
    }
}

/// Verifies pushed revisions on one resolved builder host over ssh (the
/// push-first builder doctrine: the builder only sees pushed refs).
pub struct BuildVerifier {
    host: BuilderHost,
    classifier: WireCheckClassifier,
    builder_system: OnceLock<String>,
}

impl BuildVerifier {
    /// Resolve `role` through `directory` and bind the verifier to the
    /// resulting host for the whole run, with the workspace check
    /// classifier.
    pub fn from_role(
        directory: &dyn ClusterRoleDirectory,
        role: &BuilderRole,
    ) -> Result<Self, Error> {
        Self::from_role_with_classifier(directory, role, WireCheckClassifier::workspace())
    }

    /// Resolve `role` and bind the verifier with an explicit classifier.
    pub fn from_role_with_classifier(
        directory: &dyn ClusterRoleDirectory,
        role: &BuilderRole,
        classifier: WireCheckClassifier,
    ) -> Result<Self, Error> {
        let host = directory.host_for(role)?;
        Ok(Self {
            host,
            classifier,
            builder_system: OnceLock::new(),
        })
    }

    /// Run one command on the builder host, returning stdout or the failure
    /// detail.
    fn run_on_host(&self, command: &str) -> Result<String, String> {
        let output = std::process::Command::new("ssh")
            .arg(self.host.as_str())
            .arg("--")
            .arg(command)
            .output()
            .map_err(|error| format!("ssh {}: {error}", self.host.as_str()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let excerpt: String = stderr
                .chars()
                .rev()
                .take(4000)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            return Err(format!("{command}: {excerpt}"));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// The builder's nix system, probed once per run.
    fn system(&self) -> Result<String, String> {
        if let Some(system) = self.builder_system.get() {
            return Ok(system.clone());
        }
        let system = self
            .run_on_host("nix eval --impure --raw --expr builtins.currentSystem")?
            .trim()
            .to_string();
        let _ = self.builder_system.set(system.clone());
        Ok(system)
    }

    /// The check names the flake at `reference` exposes for `system`.
    ///
    /// A repository without a checks attribute answers with an empty list
    /// — the enumeration expression treats absence as data, so the caller's
    /// default-build fallback is legitimate. An eval or transport failure
    /// is `Err` and becomes a collected verify failure: it must never
    /// silently downgrade the gate to a plain build.
    fn check_names(
        &self,
        reference: &FlakeReference,
        system: &str,
    ) -> Result<Vec<CheckName>, String> {
        let enumeration = CheckEnumeration::new(reference.clone(), system);
        let stdout = self.run_on_host(&enumeration.command())?;
        enumeration.decode(&stdout)
    }
}

/// The check-enumeration probe for one pushed revision: the eval command
/// sent to the builder and the decoding of its reply.
///
/// The expression opens the locked flake reference with `builtins.getFlake`
/// and answers `[]` when no `checks.<system>` attribute exists, so absence
/// is a first-class answer and every command failure is a genuine failure —
/// the distinction that keeps an eval/ssh breakage from masquerading as
/// "no checks" and downgrading the verify to a plain build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckEnumeration {
    reference: FlakeReference,
    system: String,
}

impl CheckEnumeration {
    pub fn new(reference: FlakeReference, system: impl Into<String>) -> Self {
        Self {
            reference,
            system: system.into(),
        }
    }

    /// The command run on the builder host.
    pub fn command(&self) -> String {
        format!(
            "nix eval --json --expr 'let flake = builtins.getFlake \"{}\"; in if flake ? checks && flake.checks ? \"{}\" then builtins.attrNames flake.checks.\"{}\" else [ ]'",
            self.reference.as_str(),
            self.system,
            self.system
        )
    }

    /// Decode the eval reply. An undecodable reply is a failure, never an
    /// empty check list.
    pub fn decode(&self, stdout: &str) -> Result<Vec<CheckName>, String> {
        serde_json::from_str::<Vec<String>>(stdout.trim())
            .map(|names| names.into_iter().map(CheckName::new).collect())
            .map_err(|error| {
                format!(
                    "check enumeration of {} undecodable: {error}: {stdout}",
                    self.reference.as_str()
                )
            })
    }
}

impl Verifier for BuildVerifier {
    fn host(&self) -> &BuilderHost {
        &self.host
    }

    fn verify(
        &self,
        forge: &Forge,
        component: &ComponentName,
        revision: &CommitIdentifier,
    ) -> VerificationOutcome {
        let reference = forge.flake_reference(component, revision);
        let system = match self.system() {
            Ok(system) => system,
            Err(detail) => {
                return VerificationOutcome::Failed(VerificationFailure { detail });
            }
        };
        let names = match self.check_names(&reference, &system) {
            Ok(names) => names,
            Err(detail) => {
                // An eval or transport failure, not an absent checks
                // attribute: fail loud rather than downgrade the gate.
                return VerificationOutcome::Failed(VerificationFailure { detail });
            }
        };
        let target = self.classifier.select(&names);
        let installables = target.installables(&reference, &system);
        let quoted: Vec<String> = installables
            .iter()
            .map(|installable| format!("'{installable}'"))
            .collect();
        let command = format!("nix build --no-link {}", quoted.join(" "));
        match self.run_on_host(&command) {
            Ok(_) => VerificationOutcome::Verified(target.gate()),
            Err(detail) => VerificationOutcome::Failed(VerificationFailure { detail }),
        }
    }
}

/// What one verification produced. A pass names the gate class that
/// passed, so a default-build pass stays visible in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    Verified(VerificationGate),
    Failed(VerificationFailure),
}

/// A failed build, with enough excerpt to diagnose without replaying the
/// build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationFailure {
    detail: String,
}

impl VerificationFailure {
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}
