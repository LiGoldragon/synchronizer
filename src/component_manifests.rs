//! The loaded pin surfaces of one component.
//!
//! Manifests are read *at the fetched remote `main` tip* through the
//! component's [`ComponentRepository`], never from the working copy: a
//! checkout may be mid-edit by an agent, and the synchronizer branch is
//! based on pushed truth. A component need not carry both layers (some
//! repos are flake-only); each present layer is a typed record.

use crate::cargo_lock::CargoLock;
use crate::cargo_manifest::{CargoManifest, GitReference};
use crate::error::Error;
use crate::flake_lock::FlakeLock;
use crate::flake_manifest::FlakeManifest;
use crate::git_repository::{ComponentRepository, RepositoryFilePath};
use crate::report::PinValue;
use crate::topology::{DependencyEdge, LocalPinName, PinLayer};
use crate::types::{BranchName, CommitIdentifier, ComponentName};

/// Both pin layers of one component, as read at its remote `main` tip.
#[derive(Debug, Clone)]
pub struct ComponentManifests {
    component: ComponentName,
    base_revision: CommitIdentifier,
    cargo: Option<CargoSurface>,
    flake: Option<FlakeSurface>,
}

impl ComponentManifests {
    /// Load whichever pin surfaces exist at `revision`. A Rust component
    /// carries manifest and lock together; a single orphan file leaves the
    /// surface absent.
    pub fn load_at(
        repository: &dyn ComponentRepository,
        component: &ComponentName,
        revision: CommitIdentifier,
    ) -> Result<Self, Error> {
        let cargo_manifest_text =
            repository.file_at(&revision, &RepositoryFilePath::cargo_manifest())?;
        let cargo_lock_text = repository.file_at(&revision, &RepositoryFilePath::cargo_lock())?;
        let cargo = match (cargo_manifest_text, cargo_lock_text) {
            (Some(manifest_text), Some(lock_text)) => Some(CargoSurface {
                manifest: CargoManifest::from_toml_text(&manifest_text, component)?,
                lock: CargoLock::from_toml_text(&lock_text, component)?,
            }),
            _ => None,
        };
        let flake_manifest_text =
            repository.file_at(&revision, &RepositoryFilePath::flake_manifest())?;
        let flake_lock_text = repository.file_at(&revision, &RepositoryFilePath::flake_lock())?;
        let flake = match (flake_manifest_text, flake_lock_text) {
            (Some(manifest_text), Some(lock_text)) => Some(FlakeSurface {
                manifest: FlakeManifest::from_nix_text(&manifest_text, component)?,
                lock: FlakeLock::from_json_text(&lock_text, component)?,
            }),
            _ => None,
        };
        Ok(Self {
            component: component.clone(),
            base_revision: revision,
            cargo,
            flake,
        })
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

    pub fn cargo_mut(&mut self) -> Option<&mut CargoSurface> {
        self.cargo.as_mut()
    }

    pub fn flake_mut(&mut self) -> Option<&mut FlakeSurface> {
        self.flake.as_mut()
    }

    /// The pin value this component currently holds for `edge`, read from
    /// the layer the edge names: an exact revision for lock layers and
    /// URL-pinned inputs, a branch reference for the Cargo manifest layer.
    pub fn pinned_value(&self, edge: &DependencyEdge) -> Result<PinValue, Error> {
        let missing = |detail: String| Error::ManifestDecode {
            component: self.component.clone(),
            layer: edge.layer(),
            detail,
        };
        match (edge.layer(), edge.local_name()) {
            (PinLayer::CargoManifest, LocalPinName::CargoPackage(name)) => {
                let surface = self
                    .cargo
                    .as_ref()
                    .ok_or_else(|| missing("cargo surface absent for a cargo edge".to_string()))?;
                let (_, source) = surface
                    .manifest
                    .git_dependencies()
                    .into_iter()
                    .find(|(dependency, _)| dependency == name)
                    .ok_or_else(|| missing(format!("no git dependency {}", name.as_str())))?;
                Ok(match source.reference() {
                    GitReference::Branch(branch) => PinValue::Reference(branch.clone()),
                    GitReference::Tag(tag) => PinValue::Reference(BranchName::new(tag.clone())),
                    GitReference::Revision(revision) => PinValue::Revision(revision.clone()),
                    GitReference::DefaultBranch => PinValue::Reference(BranchName::main()),
                })
            }
            (PinLayer::CargoLock, LocalPinName::CargoPackage(name)) => {
                let surface = self
                    .cargo
                    .as_ref()
                    .ok_or_else(|| missing("cargo surface absent for a cargo edge".to_string()))?;
                let (_, pin) = surface
                    .lock
                    .git_packages()?
                    .into_iter()
                    .find(|(package, _)| package == name)
                    .ok_or_else(|| missing(format!("no locked git package {}", name.as_str())))?;
                Ok(PinValue::Revision(pin.revision().clone()))
            }
            (PinLayer::FlakeManifest, LocalPinName::FlakeInput(input)) => {
                let surface = self
                    .flake
                    .as_ref()
                    .ok_or_else(|| missing("flake surface absent for a flake edge".to_string()))?;
                let occurrence = surface
                    .manifest
                    .pinned_inputs()
                    .into_iter()
                    .find(|occurrence| occurrence.input() == input)
                    .ok_or_else(|| missing(format!("no pinned input url {}", input.as_str())))?;
                match occurrence.url() {
                    crate::flake_manifest::InputUrl::GitHub {
                        pin: crate::flake_manifest::GitHubPin::Pinned(segment),
                        ..
                    } => Ok(PinValue::Revision(CommitIdentifier::new(segment.clone()))),
                    _ => Err(missing(format!(
                        "input {} carries no revision pin",
                        input.as_str()
                    ))),
                }
            }
            (PinLayer::FlakeLock, LocalPinName::FlakeInput(input)) => {
                let surface = self
                    .flake
                    .as_ref()
                    .ok_or_else(|| missing("flake surface absent for a flake edge".to_string()))?;
                let (_, locked) = surface
                    .lock
                    .github_inputs()
                    .into_iter()
                    .find(|(name, _)| name == input)
                    .ok_or_else(|| missing(format!("no locked github input {}", input.as_str())))?;
                let revision = locked
                    .revision()
                    .ok_or_else(|| missing(format!("input {} locks no rev", input.as_str())))?;
                Ok(PinValue::Revision(revision))
            }
            (layer, name) => Err(missing(format!(
                "edge layer {layer:?} does not match local pin name {name:?}"
            ))),
        }
    }
}

/// The Cargo pin layer: manifest plus lock.
#[derive(Debug, Clone)]
pub struct CargoSurface {
    manifest: CargoManifest,
    lock: CargoLock,
}

impl CargoSurface {
    pub fn new(manifest: CargoManifest, lock: CargoLock) -> Self {
        Self { manifest, lock }
    }

    pub fn manifest(&self) -> &CargoManifest {
        &self.manifest
    }

    pub fn lock(&self) -> &CargoLock {
        &self.lock
    }

    pub fn manifest_mut(&mut self) -> &mut CargoManifest {
        &mut self.manifest
    }

    pub fn lock_mut(&mut self) -> &mut CargoLock {
        &mut self.lock
    }
}

/// The flake pin layer: manifest plus lock.
#[derive(Debug, Clone)]
pub struct FlakeSurface {
    manifest: FlakeManifest,
    lock: FlakeLock,
}

impl FlakeSurface {
    pub fn new(manifest: FlakeManifest, lock: FlakeLock) -> Self {
        Self { manifest, lock }
    }

    pub fn manifest(&self) -> &FlakeManifest {
        &self.manifest
    }

    pub fn lock(&self) -> &FlakeLock {
        &self.lock
    }

    pub fn manifest_mut(&mut self) -> &mut FlakeManifest {
        &mut self.manifest
    }

    pub fn lock_mut(&mut self) -> &mut FlakeLock {
        &mut self.lock
    }
}
