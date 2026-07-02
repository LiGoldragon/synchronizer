//! Typed model of a component's `Cargo.toml`.
//!
//! Deserialized with serde through the TOML deserializer — never string
//! munging. Tables the synchronizer does not manipulate are preserved as
//! typed TOML values in `remainder` fields and reserialized untouched.
//! Reserialization goes through [`crate::toml_pretty::PrettyPrinter`] and
//! produces the canonical workspace manifest style; TOML comments are not
//! preserved (see ARCHITECTURE.md §4 and §14).
//!
//! Field names follow the external Cargo format at the serde boundary.

use serde::{Deserialize, Serialize};
use toml::Table;

use crate::error::Error;
use crate::toml_pretty::PrettyPrinter;
use crate::types::{BranchName, CommitIdentifier, RepositoryUrl, TomlText};

/// A dependency key in a Cargo dependency table. This is a *package* name,
/// which may differ from the repository name (`nota` lives in `nota-next`);
/// component matching goes through the git URL, never this name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DependencyName(String);

impl DependencyName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A `Cargo.toml` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoManifest {
    package: PackageSection,
    #[serde(default)]
    dependencies: DependencyTable,
    #[serde(default, rename = "build-dependencies")]
    build_dependencies: DependencyTable,
    #[serde(default, rename = "dev-dependencies")]
    development_dependencies: DependencyTable,
    /// Every table this tool does not manipulate ([lib], [[bin]], [lints],
    /// [features], [patch], target tables, ...), preserved as typed TOML
    /// values.
    #[serde(flatten)]
    remainder: Table,
}

impl CargoManifest {
    /// Deserialize a manifest from TOML text.
    pub fn from_toml_text(text: &str) -> Result<Self, Error> {
        todo!("toml deserializer via serde")
    }

    /// The `[package] name` value.
    pub fn package_name(&self) -> &DependencyName {
        todo!()
    }

    /// The `[package] version` value.
    pub fn package_version(&self) -> &PackageVersion {
        todo!()
    }

    /// Every git dependency across the dependency, build-dependency, and
    /// dev-dependency tables. Topology discovery matches these against the
    /// configured component set by repository URL.
    pub fn git_dependencies(&self) -> Vec<(DependencyName, GitSource)> {
        todo!()
    }

    /// Redirect the named git dependency to `reference` in-type, returning
    /// the previous reference.
    ///
    /// This is the cascade edit: a consumer pinning a bumped dependency is
    /// redirected from `branch = "main"` to `branch = "synchronizer"` so
    /// Cargo can reach the locked revision on a fresh clone.
    pub fn redirect_git_dependency(
        &mut self,
        name: &DependencyName,
        reference: GitReference,
    ) -> Result<GitReference, Error> {
        todo!()
    }

    /// Reserialize through the pretty printer in the canonical workspace
    /// manifest style.
    pub fn to_pretty_toml(&self, printer: &PrettyPrinter) -> Result<TomlText, Error> {
        todo!()
    }
}

/// The `[package]` table. Fields the tool reads are typed; the rest is
/// preserved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageSection {
    name: DependencyName,
    version: PackageVersion,
    #[serde(flatten)]
    remainder: Table,
}

/// A Cargo package version string, e.g. `0.2.0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageVersion(String);

impl PackageVersion {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One `[dependencies]`-shaped table, in declaration order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DependencyTable(Table);

/// A parsed view of one dependency entry's git source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    url: RepositoryUrl,
    reference: GitReference,
}

impl GitSource {
    pub fn url(&self) -> &RepositoryUrl {
        &self.url
    }

    pub fn reference(&self) -> &GitReference {
        &self.reference
    }
}

/// How a git dependency names the commit it follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitReference {
    Branch(BranchName),
    Revision(CommitIdentifier),
}
