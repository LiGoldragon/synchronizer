//! Build verification of a pushed bump.
//!
//! After a component's synchronizer branch is pushed, its build is verified
//! *at the pushed revision, addressed remotely* (`github:` flake
//! reference), on the host resolved from the configured builder role — so
//! the verify exercises exactly what a fresh consumer would fetch, and the
//! builder sees only pushed truth. The narrow check is the default
//! package build; wide flake checks are deliberately not the verify gate.
//!
//! A verification failure is report data, not a crate [`Error`]: the ascent
//! continues and the failure is collected.

use crate::configuration::Forge;
use crate::error::Error;
use crate::role_resolution::ClusterRoleDirectory;
use crate::types::{BuilderHost, BuilderRole, CommitIdentifier, ComponentName};

/// Verifies pushed revisions on one resolved builder host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildVerifier {
    host: BuilderHost,
}

impl BuildVerifier {
    /// Resolve `role` through `directory` and bind the verifier to the
    /// resulting host for the whole run.
    pub fn from_role(
        directory: &dyn ClusterRoleDirectory,
        role: &BuilderRole,
    ) -> Result<Self, Error> {
        todo!()
    }

    pub fn host(&self) -> &BuilderHost {
        &self.host
    }

    /// Build the component's default package at `revision` on the builder
    /// host, addressed as a remote flake reference
    /// (`github:<owner>/<component>/<revision>#default`), and report the
    /// outcome. Never returns `Err` for a build failure — that is a
    /// collected outcome.
    pub fn verify(
        &self,
        forge: &Forge,
        component: &ComponentName,
        revision: &CommitIdentifier,
    ) -> VerificationOutcome {
        todo!("ssh <host> nix build <reference> — execution shape per ARCHITECTURE.md §8")
    }
}

/// What one verification produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    Verified,
    Failed(VerificationFailure),
}

/// A failed build, with enough excerpt to diagnose without replaying the
/// build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationFailure {
    detail: String,
}

impl VerificationFailure {
    pub fn detail(&self) -> &str {
        &self.detail
    }
}
