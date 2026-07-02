//! NOTA configuration document.
//!
//! The configuration names the participating components and where their
//! local clones live; it never declares dependency edges (topology is
//! discovered from manifests) and never names a builder host (only a
//! CriomOS role). Decoding goes through the canonical NOTA codec only.
//!
//! Schema (strict positional; the root record is an untagged struct per
//! the canonical codec — the `SynchronizerConfig` label in ARCHITECTURE.md
//! §3 is schema documentation, not a wire tag):
//!
//! ```nota
//! (<forge> <checkout-root> [<component>] <builder-role> <cluster-configuration>)
//! ```

use std::path::{Path, PathBuf};

use nota::{NotaDecode, NotaEncode, NotaSource};

use crate::error::Error;
use crate::types::{AbsolutePath, BuilderRole, ComponentName, RepositoryUrl};

/// Root configuration document, decoded from NOTA text.
///
/// Field order is the positional wire order; reordering is a compatibility
/// change.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct SynchronizerConfig {
    forge: Forge,
    checkout_root: AbsolutePath,
    components: Vec<Component>,
    builder_role: BuilderRole,
    cluster_configuration: ClusterConfiguration,
}

impl SynchronizerConfig {
    pub fn new(
        forge: Forge,
        checkout_root: AbsolutePath,
        components: Vec<Component>,
        builder_role: BuilderRole,
        cluster_configuration: ClusterConfiguration,
    ) -> Self {
        Self {
            forge,
            checkout_root,
            components,
            builder_role,
            cluster_configuration,
        }
    }

    /// Decode a configuration document from NOTA text via the canonical
    /// codec.
    pub fn from_nota_text(text: &str) -> Result<Self, Error> {
        NotaSource::new(text)
            .parse::<Self>()
            .map_err(|error| Error::ConfigurationDecode {
                detail: error.to_string(),
            })
    }

    /// Encode the configuration as canonical NOTA text.
    pub fn to_nota_text(&self) -> String {
        self.to_nota()
    }

    /// Read and decode the configuration file at `path`.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text =
            std::fs::read_to_string(path).map_err(|source| Error::ConfigurationUnreadable {
                path: path.to_path_buf(),
                source,
            })?;
        Self::from_nota_text(&text)
    }

    pub fn forge(&self) -> &Forge {
        &self.forge
    }

    pub fn components(&self) -> &[Component] {
        &self.components
    }

    pub fn builder_role(&self) -> &BuilderRole {
        &self.builder_role
    }

    pub fn cluster_configuration(&self) -> &ClusterConfiguration {
        &self.cluster_configuration
    }

    fn component(&self, name: &ComponentName) -> Result<&Component, Error> {
        self.components
            .iter()
            .find(|component| component.name() == name)
            .ok_or_else(|| Error::UnknownComponent(name.clone()))
    }

    /// Absolute checkout path of `component`, resolving `AtRoot` against the
    /// checkout root.
    pub fn checkout_path(&self, component: &ComponentName) -> Result<PathBuf, Error> {
        let component = self.component(component)?;
        Ok(match component.checkout() {
            ComponentCheckout::AtRoot => self
                .checkout_root
                .as_path_buffer()
                .join(component.name().as_str()),
            ComponentCheckout::AtPath(path) => path.as_path_buffer(),
        })
    }

    /// Remote URL of `component` on the configured forge.
    pub fn repository_url(&self, component: &ComponentName) -> Result<RepositoryUrl, Error> {
        let component = self.component(component)?;
        Ok(self.forge.repository_url(component.name()))
    }
}

/// The forge holding every component remote.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum Forge {
    GitHub(ForgeOwner),
}

impl Forge {
    /// The clone/push URL of `repository` on this forge.
    pub fn repository_url(&self, repository: &ComponentName) -> RepositoryUrl {
        match self {
            Self::GitHub(owner) => RepositoryUrl::new(format!(
                "https://github.com/{}/{}.git",
                owner.as_str(),
                repository.as_str()
            )),
        }
    }

    /// The account or organization owning every component remote, used to
    /// match flake input sources by owner and repository.
    pub fn owner(&self) -> &ForgeOwner {
        match self {
            Self::GitHub(owner) => owner,
        }
    }

    /// The remote flake reference of `component` at `revision` — how the
    /// verify addresses pushed truth.
    pub fn flake_reference(
        &self,
        component: &ComponentName,
        revision: &crate::types::CommitIdentifier,
    ) -> crate::types::FlakeReference {
        match self {
            Self::GitHub(owner) => crate::types::FlakeReference::new(format!(
                "github:{}/{}/{}",
                owner.as_str(),
                component.as_str(),
                revision.as_str()
            )),
        }
    }
}

/// A forge account or organization name, e.g. `LiGoldragon`.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct ForgeOwner(String);

impl ForgeOwner {
    pub fn new(owner: impl Into<String>) -> Self {
        Self(owner.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One participating repository.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct Component {
    name: ComponentName,
    checkout: ComponentCheckout,
}

impl Component {
    pub fn new(name: ComponentName, checkout: ComponentCheckout) -> Self {
        Self { name, checkout }
    }

    pub fn name(&self) -> &ComponentName {
        &self.name
    }

    pub fn checkout(&self) -> &ComponentCheckout {
        &self.checkout
    }
}

/// Where a component's local clone lives.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum ComponentCheckout {
    /// `<checkout-root>/<name>` — the ghq-style default.
    AtRoot,
    /// An explicit absolute path overriding the root convention.
    AtPath(AbsolutePath),
}

/// Where CriomOS builder roles resolve to hosts.
///
/// The confirmed authoritative surface (ARCHITECTURE.md §8, §14 q5) is the
/// cluster proposal document — the horizon-rs `ClusterProposal` NOTA datom
/// (e.g. `goldragon/datom.nota`) whose per-node `services` vectors author
/// every role in the cluster. Cluster flakes carry no role→host output and
/// the production cluster repository is not a flake; Lojix records
/// deployment generations, not roles.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum ClusterConfiguration {
    /// A cluster proposal document authored in the horizon-rs
    /// `ClusterProposal` schema.
    ClusterProposal(AbsolutePath),
}
