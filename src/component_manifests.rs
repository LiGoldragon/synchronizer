//! The loaded pin surfaces of one component.
//!
//! Manifests are read *at the fetched remote `main` tip* through the
//! component's [`GitRepository`], never from the working copy: a checkout
//! may be mid-edit by an agent, and the synchronizer branch is based on
//! pushed truth. A component need not carry both layers (some repos are
//! flake-only); each present layer is a typed record.

use crate::cargo_lock::CargoLock;
use crate::cargo_manifest::CargoManifest;
use crate::error::Error;
use crate::flake_lock::FlakeLock;
use crate::flake_manifest::FlakeManifest;
use crate::git_repository::GitRepository;
use crate::topology::DependencyEdge;
use crate::types::{CommitIdentifier, ComponentName};

/// Both pin layers of one component, as read at its remote `main` tip.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentManifests {
    component: ComponentName,
    base_revision: CommitIdentifier,
    cargo: Option<CargoSurface>,
    flake: Option<FlakeSurface>,
}

impl ComponentManifests {
    /// Load whichever pin surfaces exist at `revision`.
    pub fn load_at(repository: &GitRepository, revision: CommitIdentifier) -> Result<Self, Error> {
        todo!(
            "read Cargo.toml/Cargo.lock/flake.nix/flake.lock at revision; absent files disable their surface"
        )
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    /// The remote `main` tip these manifests were read at — the base of any
    /// bump commit for this component.
    pub fn base_revision(&self) -> &CommitIdentifier {
        &self.base_revision
    }

    pub fn cargo(&self) -> Option<&CargoSurface> {
        self.cargo.as_ref()
    }

    pub fn flake(&self) -> Option<&FlakeSurface> {
        self.flake.as_ref()
    }

    /// The revision this component currently pins for `edge`, read from the
    /// layer the edge names.
    pub fn pinned_revision(&self, edge: &DependencyEdge) -> Result<CommitIdentifier, Error> {
        todo!()
    }
}

/// The Cargo pin layer: manifest plus lock. A Rust component always carries
/// both.
#[derive(Debug, Clone, PartialEq)]
pub struct CargoSurface {
    manifest: CargoManifest,
    lock: CargoLock,
}

impl CargoSurface {
    pub fn manifest(&self) -> &CargoManifest {
        &self.manifest
    }

    pub fn lock(&self) -> &CargoLock {
        &self.lock
    }
}

/// The flake pin layer: manifest plus lock.
#[derive(Debug, Clone, PartialEq)]
pub struct FlakeSurface {
    manifest: FlakeManifest,
    lock: FlakeLock,
}

impl FlakeSurface {
    pub fn manifest(&self) -> &FlakeManifest {
        &self.manifest
    }

    pub fn lock(&self) -> &FlakeLock {
        &self.lock
    }
}
