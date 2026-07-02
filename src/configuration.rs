//! NOTA configuration document.
//!
//! The configuration carries every project-specific fact the tool needs; the
//! tool source carries none. It names the participating components and where
//! their local clones live, the branch scheme (which mainline branch is the
//! version target and which staging branch the tool owns), how the
//! build-verify host is determined, how the verify gate is selected, and the
//! commit author. It never declares dependency edges (topology is discovered
//! from manifests) and never names a builder host directly unless the
//! configuration itself chooses the direct-host strategy. Decoding goes
//! through the canonical NOTA codec only.
//!
//! Schema (strict positional; the root record is an untagged struct per
//! the canonical codec — the `SynchronizerConfig` label in ARCHITECTURE.md
//! §3 is schema documentation, not a wire tag):
//!
//! ```nota
//! (<forge>
//!  <checkout-root>
//!  [<component>]
//!  <branch-scheme>
//!  <builder-resolution>
//!  <verify-policy>
//!  <commit-author>)
//! ```

use std::path::{Path, PathBuf};

use nota::{NotaDecode, NotaEncode, NotaSource};

use crate::build_verify::VerifyPolicy;
use crate::error::Error;
use crate::types::{
    AbsolutePath, AuthorEmail, AuthorName, BranchName, BuilderHost, BuilderRole, ComponentName,
    RepositoryUrl,
};

/// Root configuration document, decoded from NOTA text.
///
/// Field order is the positional wire order; reordering is a compatibility
/// change.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct SynchronizerConfig {
    forge: Forge,
    checkout_root: AbsolutePath,
    components: Vec<Component>,
    branch_scheme: BranchScheme,
    builder_resolution: BuilderResolution,
    verify_policy: VerifyPolicy,
    commit_author: CommitAuthor,
}

impl SynchronizerConfig {
    pub fn new(
        forge: Forge,
        checkout_root: AbsolutePath,
        components: Vec<Component>,
        branch_scheme: BranchScheme,
        builder_resolution: BuilderResolution,
        verify_policy: VerifyPolicy,
        commit_author: CommitAuthor,
    ) -> Self {
        Self {
            forge,
            checkout_root,
            components,
            branch_scheme,
            builder_resolution,
            verify_policy,
            commit_author,
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

    pub fn branch_scheme(&self) -> &BranchScheme {
        &self.branch_scheme
    }

    pub fn builder_resolution(&self) -> &BuilderResolution {
        &self.builder_resolution
    }

    pub fn verify_policy(&self) -> &VerifyPolicy {
        &self.verify_policy
    }

    pub fn commit_author(&self) -> &CommitAuthor {
        &self.commit_author
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

/// The branch scheme a run works against.
///
/// Both names are configuration, not tool constants: a consumer whose default
/// branch is `master` or whose staging branch must avoid a collision names
/// them here. Nothing in the tool assumes `main` or `synchronizer`.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct BranchScheme {
    /// The mainline branch: the default version target for a dependency not
    /// bumped this run, and the branch a cascade redirect moves *away* from.
    mainline: BranchName,
    /// The tool-owned staging branch every bump is committed to and
    /// force-pushed to — never `main`/mainline.
    staging: BranchName,
}

impl BranchScheme {
    pub fn new(mainline: BranchName, staging: BranchName) -> Self {
        Self { mainline, staging }
    }

    pub fn mainline(&self) -> &BranchName {
        &self.mainline
    }

    pub fn staging(&self) -> &BranchName {
        &self.staging
    }
}

/// How the build-verify host is determined — the generic strategy interface.
///
/// The CriomOS cluster-datom resolver is one optional strategy
/// (`ClusterRole` + [`ClusterSource::ClusterProposal`]), never the only path
/// and never hard-coded: a consumer with no cluster directory names the host
/// directly with `DirectHost`. Growth to further strategies happens by new
/// variant.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum BuilderResolution {
    /// A literal builder host — no cluster, no role indirection.
    DirectHost(BuilderHost),
    /// Resolve a role to a host through a cluster source.
    ClusterRole(BuilderRole, ClusterSource),
}

/// Where a role resolves to a host — the cluster-directory strategy set.
///
/// The confirmed CriomOS surface is the cluster proposal document — the
/// horizon-rs `ClusterProposal` NOTA datom (e.g. `goldragon/datom.nota`)
/// whose per-node `services` vectors author every role in the cluster.
/// Cluster flakes carry no role→host output and the production cluster
/// repository is not a flake; Lojix records deployment generations, not
/// roles. This is a pluggable set: other cluster surfaces join by new
/// variant, and the whole strategy is optional (see
/// [`BuilderResolution::DirectHost`]).
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum ClusterSource {
    /// A cluster proposal document authored in the horizon-rs
    /// `ClusterProposal` schema.
    ClusterProposal(AbsolutePath),
}

/// The author and committer identity stamped on every bump commit.
///
/// Configuration, not a tool constant: the tool holds no name or email of its
/// own (no `synchronizer@criome.net` baked in). A consumer names their own CI
/// identity here.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct CommitAuthor {
    name: AuthorName,
    email: AuthorEmail,
}

impl CommitAuthor {
    pub fn new(name: AuthorName, email: AuthorEmail) -> Self {
        Self { name, email }
    }

    pub fn name(&self) -> &AuthorName {
        &self.name
    }

    pub fn email(&self) -> &AuthorEmail {
        &self.email
    }
}
