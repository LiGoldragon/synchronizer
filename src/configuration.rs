//! NOTA configuration document.
//!
//! The configuration names the participating components and where their
//! local clones live; it never declares dependency edges (topology is
//! discovered from manifests) and never names a builder host (only a
//! CriomOS role). Decoding goes through the canonical NOTA codec only.
//!
//! Schema (strict positional; see ARCHITECTURE.md §3):
//!
//! ```nota
//! (SynchronizerConfig <forge> <checkout-root> [<component>] <builder-role> <cluster-configuration>)
//! ```

use std::path::{Path, PathBuf};

use nota::{NotaDecode, NotaEncode};

use crate::error::Error;
use crate::types::{AbsolutePath, BuilderRole, ComponentName, FlakeReference, RepositoryUrl};

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
    /// Decode a configuration document from NOTA text via the canonical
    /// codec.
    pub fn from_nota_text(text: &str) -> Result<Self, Error> {
        todo!("decode through the canonical nota codec; no hand-rolled parsing")
    }

    /// Read and decode the configuration file at `path`.
    pub fn load(path: &Path) -> Result<Self, Error> {
        todo!("read file, delegate to from_nota_text")
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

    /// Absolute checkout path of `component`, resolving `AtRoot` against the
    /// checkout root.
    pub fn checkout_path(&self, component: &ComponentName) -> Result<PathBuf, Error> {
        todo!("AtRoot => <checkout-root>/<name>; AtPath => the explicit path")
    }

    /// Remote URL of `component` on the configured forge.
    pub fn repository_url(&self, component: &ComponentName) -> Result<RepositoryUrl, Error> {
        todo!("https://github.com/<owner>/<name>.git for (GitHub <owner>)")
    }
}

/// The forge holding every component remote.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum Forge {
    GitHub(ForgeOwner),
}

/// A forge account or organization name, e.g. `LiGoldragon`.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct ForgeOwner(String);

impl ForgeOwner {
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
/// The variant set is expected to grow or change once the concrete
/// cluster-config surface is confirmed with OS-ops (see ARCHITECTURE.md
/// §14); `ClusterFlake` is the planned first shape.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum ClusterConfiguration {
    /// A flake whose outputs define the cluster's role-to-host mapping.
    ClusterFlake(FlakeReference),
}
