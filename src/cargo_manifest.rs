//! Typed model of a component's `Cargo.toml`.
//!
//! Reading goes through serde and the TOML deserializer — the read model
//! feeds topology discovery and staleness. Writing goes through the
//! format-preserving document ([`crate::toml_document::PreservedTomlDocument`],
//! psyche-locked): the cascade redirect edits exactly one dependency value
//! and leaves every comment and layout byte untouched.
//!
//! Field names follow the external Cargo format at the serde boundary.

use serde::Deserialize;
use toml::Table;

use crate::error::{Error, UnbumpablePinReason};
use crate::toml_document::PreservedTomlDocument;
use crate::topology::PinLayer;
use crate::types::{BranchName, CommitIdentifier, ComponentName, RepositoryUrl, TomlText};

/// A resolved *package* name: the dependency table key with a `package =`
/// rename applied. It may differ from both the table key (`nota-next =
/// { package = "nota", ... }`) and the repository name; component matching
/// goes through the git URL, never this name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct DependencyName(String);

impl DependencyName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The literal key of one entry in a Cargo dependency table — the name the
/// TOML document is addressed by. Under a `package =` rename it differs
/// from the [`DependencyName`] the entry resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyKey(String);

impl DependencyKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The three dependency tables a manifest may hold, in external spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyTableKind {
    Dependencies,
    BuildDependencies,
    DevelopmentDependencies,
}

impl DependencyTableKind {
    pub const ALL: [Self; 3] = [
        Self::Dependencies,
        Self::BuildDependencies,
        Self::DevelopmentDependencies,
    ];

    /// The external table key in `Cargo.toml`.
    pub fn table_key(&self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::BuildDependencies => "build-dependencies",
            Self::DevelopmentDependencies => "dev-dependencies",
        }
    }
}

/// A `Cargo.toml` document: the serde read model plus the
/// format-preserving write document, parsed from the same text.
#[derive(Debug, Clone)]
pub struct CargoManifest {
    component: ComponentName,
    read: CargoManifestModel,
    document: PreservedTomlDocument,
}

impl CargoManifest {
    /// Deserialize a manifest from TOML text.
    pub fn from_toml_text(text: &str, component: &ComponentName) -> Result<Self, Error> {
        let read: CargoManifestModel =
            toml::from_str(text).map_err(|error| Error::ManifestDecode {
                component: component.clone(),
                layer: PinLayer::CargoManifest,
                detail: error.to_string(),
            })?;
        let document = PreservedTomlDocument::parse(text, component, PinLayer::CargoManifest)?;
        Ok(Self {
            component: component.clone(),
            read,
            document,
        })
    }

    /// The `[package] name` value, `None` for a virtual workspace manifest.
    pub fn package_name(&self) -> Option<&DependencyName> {
        self.read.package.as_ref().map(|package| &package.name)
    }

    /// The `[package] version` value, `None` for a virtual workspace
    /// manifest or a version inherited from a workspace.
    pub fn package_version(&self) -> Option<&PackageVersion> {
        self.read
            .package
            .as_ref()
            .and_then(|package| package.version.as_ref())
    }

    /// Every git dependency across the dependency, build-dependency, and
    /// dev-dependency tables. Topology discovery matches these against the
    /// configured component set by repository URL.
    pub fn git_dependencies(&self) -> Vec<(DependencyName, GitSource)> {
        DependencyTableKind::ALL
            .iter()
            .flat_map(|kind| self.read.table(*kind).git_entries())
            .collect()
    }

    /// Redirect every git dependency entry resolving to `name` to
    /// `reference` in the format-preserving document, returning the previous
    /// reference the entries shared.
    ///
    /// This is the cascade edit: a consumer pinning a bumped dependency is
    /// redirected from the mainline branch to the configured staging branch
    /// so Cargo can reach the locked revision on a fresh clone. A producer
    /// may be declared under several same-name entries (the same crate in
    /// `[dependencies]` and `[dev-dependencies]`, or two keys sharing a
    /// `package =` rename); every such entry follows the one producer, so
    /// every match is redirected coherently — one edge, all its textual
    /// entries. Each entry is addressed by its own table *key*, which under
    /// a `package =` rename differs from the resolved package name, never by
    /// the shared name. Comments and layout survive: only the branch values
    /// change.
    ///
    /// A deliberately rev- or tag-pinned entry fails loud
    /// ([`Error::UnbumpablePin`]) and leaves the manifest untouched:
    /// inserting `branch` beside `rev` would emit an invalid manifest, and a
    /// deliberate pin is a choice the mechanical bump must not override.
    pub fn redirect_git_dependency(
        &mut self,
        name: &DependencyName,
        reference: GitReference,
    ) -> Result<GitReference, Error> {
        let matches: Vec<(DependencyTableKind, DependencyKey, GitSource)> = self
            .git_dependencies_with_tables()
            .into_iter()
            .filter(|(_, _, entry_name, _)| entry_name == name)
            .map(|(kind, key, _, source)| (kind, key, source))
            .collect();
        let Some((_, _, first)) = matches.first() else {
            return Err(Error::NotComponentDependency {
                consumer: self.component.clone(),
                dependency: name.as_str().to_string(),
            });
        };
        let previous = first.reference.clone();
        // Every same-name entry must be branch- or default-branch-declared:
        // a deliberate rev/tag pin among them cannot be branch-redirected,
        // so the whole redirect fails loud and the manifest stays untouched.
        for (_, _, source) in &matches {
            match &source.reference {
                GitReference::Revision(_) => {
                    return Err(Error::UnbumpablePin {
                        consumer: self.component.clone(),
                        dependency: name.as_str().to_string(),
                        reason: UnbumpablePinReason::DeliberateRevisionPin,
                    });
                }
                GitReference::Tag(_) => {
                    return Err(Error::UnbumpablePin {
                        consumer: self.component.clone(),
                        dependency: name.as_str().to_string(),
                        reason: UnbumpablePinReason::DeliberateTagPin,
                    });
                }
                GitReference::Branch(_) | GitReference::DefaultBranch => {}
            }
        }
        let branch = match &reference {
            GitReference::Branch(branch) => branch.as_str().to_string(),
            other => {
                return Err(Error::ManifestEncode {
                    component: self.component.clone(),
                    layer: PinLayer::CargoManifest,
                    detail: format!("cascade redirect requires a branch reference, got {other:?}"),
                });
            }
        };
        for (kind, key, _) in &matches {
            let document = self.document.as_document_mut();
            let entry = document
                .get_mut(kind.table_key())
                .and_then(|table| table.get_mut(key.as_str()))
                .ok_or_else(|| Error::ManifestEncode {
                    component: self.component.clone(),
                    layer: PinLayer::CargoManifest,
                    detail: format!("dependency key {} not found in document", key.as_str()),
                })?;
            match entry {
                toml_edit::Item::Value(toml_edit::Value::InlineTable(table)) => {
                    table.insert("branch", toml_edit::Value::from(branch.clone()));
                }
                toml_edit::Item::Table(table) => {
                    table.insert("branch", toml_edit::value(branch.clone()));
                }
                other => {
                    return Err(Error::ManifestEncode {
                        component: self.component.clone(),
                        layer: PinLayer::CargoManifest,
                        detail: format!(
                            "dependency key {} is not a table entry: {other:?}",
                            key.as_str()
                        ),
                    });
                }
            }
        }
        // Keep the read model coherent with the document across every table
        // the same-name entries live in.
        for kind in DependencyTableKind::ALL {
            self.read
                .table_mut(kind)
                .set_branch(name, reference.clone());
        }
        Ok(previous)
    }

    /// The document text with all edits applied, preserving comments and
    /// layout of everything untouched.
    pub fn to_toml_text(&self) -> TomlText {
        self.document.to_toml_text()
    }

    /// Every dependency package name this manifest declares across the
    /// dependency and build-dependency tables (the sets a consumer's lock
    /// records for this package), with `package =` renames applied.
    /// Dev-dependencies stay out: a consumer's lock never records them for
    /// a dependency.
    pub fn declared_dependency_package_names(&self) -> Vec<DependencyName> {
        [
            DependencyTableKind::Dependencies,
            DependencyTableKind::BuildDependencies,
        ]
        .iter()
        .flat_map(|kind| self.read.table(*kind).package_names())
        .collect()
    }

    fn git_dependencies_with_tables(
        &self,
    ) -> Vec<(
        DependencyTableKind,
        DependencyKey,
        DependencyName,
        GitSource,
    )> {
        DependencyTableKind::ALL
            .iter()
            .flat_map(|kind| {
                self.read
                    .table(*kind)
                    .git_entries_with_keys()
                    .into_iter()
                    .map(move |(key, name, source)| (*kind, key, name, source))
            })
            .collect()
    }
}

/// The serde read model of a manifest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct CargoManifestModel {
    #[serde(default)]
    package: Option<PackageSection>,
    #[serde(default)]
    dependencies: DependencyTable,
    #[serde(default, rename = "build-dependencies")]
    build_dependencies: DependencyTable,
    #[serde(default, rename = "dev-dependencies")]
    development_dependencies: DependencyTable,
    /// Every table this tool does not read ([lib], [[bin]], [lints],
    /// [features], [workspace], target tables, ...), preserved as typed
    /// TOML values for completeness of the read model.
    #[serde(flatten)]
    #[allow(dead_code)]
    remainder: Table,
}

impl CargoManifestModel {
    fn table(&self, kind: DependencyTableKind) -> &DependencyTable {
        match kind {
            DependencyTableKind::Dependencies => &self.dependencies,
            DependencyTableKind::BuildDependencies => &self.build_dependencies,
            DependencyTableKind::DevelopmentDependencies => &self.development_dependencies,
        }
    }

    fn table_mut(&mut self, kind: DependencyTableKind) -> &mut DependencyTable {
        match kind {
            DependencyTableKind::Dependencies => &mut self.dependencies,
            DependencyTableKind::BuildDependencies => &mut self.build_dependencies,
            DependencyTableKind::DevelopmentDependencies => &mut self.development_dependencies,
        }
    }
}

/// The `[package]` table. Fields the tool reads are typed; the rest is
/// ignored by the read model (the write document preserves everything).
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PackageSection {
    name: DependencyName,
    /// Missing or non-string (`version.workspace = true`) versions decode
    /// as `None`.
    #[serde(default)]
    version: Option<PackageVersion>,
}

/// A Cargo package version string, e.g. `0.2.0`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackageVersion(String);

impl PackageVersion {
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One `[dependencies]`-shaped table, in declaration order.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct DependencyTable(Table);

impl DependencyTable {
    /// The git-sourced entries of this table: resolved package name (the
    /// key with a `package =` rename applied) and parsed git source.
    fn git_entries(&self) -> Vec<(DependencyName, GitSource)> {
        self.git_entries_with_keys()
            .into_iter()
            .map(|(_, name, source)| (name, source))
            .collect()
    }

    /// The git-sourced entries of this table with their literal table keys
    /// — the names the format-preserving document is addressed by.
    fn git_entries_with_keys(&self) -> Vec<(DependencyKey, DependencyName, GitSource)> {
        self.0
            .iter()
            .filter_map(|(key, value)| {
                let table = value.as_table()?;
                let url = table.get("git")?.as_str()?;
                let package = table
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key);
                let reference = if let Some(branch) = table.get("branch").and_then(|v| v.as_str()) {
                    GitReference::Branch(BranchName::new(branch))
                } else if let Some(tag) = table.get("tag").and_then(|v| v.as_str()) {
                    GitReference::Tag(tag.to_string())
                } else if let Some(revision) = table.get("rev").and_then(|v| v.as_str()) {
                    GitReference::Revision(CommitIdentifier::new(revision))
                } else {
                    GitReference::DefaultBranch
                };
                Some((
                    DependencyKey::new(key),
                    DependencyName::new(package),
                    GitSource {
                        url: RepositoryUrl::new(url),
                        reference,
                    },
                ))
            })
            .collect()
    }

    /// Every dependency package name in this table: plain-version entries
    /// keep their key, table entries apply a `package =` rename.
    fn package_names(&self) -> Vec<DependencyName> {
        self.0
            .iter()
            .map(|(key, value)| {
                let package = value
                    .as_table()
                    .and_then(|table| table.get("package"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key);
                DependencyName::new(package)
            })
            .collect()
    }

    fn set_branch(&mut self, name: &DependencyName, reference: GitReference) {
        if let GitReference::Branch(branch) = reference {
            for (key, value) in self.0.iter_mut() {
                let is_entry = {
                    let Some(table) = value.as_table() else {
                        continue;
                    };
                    let package = table
                        .get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(key);
                    table.contains_key("git") && package == name.as_str()
                };
                if is_entry && let Some(table) = value.as_table_mut() {
                    table.insert(
                        "branch".to_string(),
                        toml::Value::String(branch.as_str().to_string()),
                    );
                }
            }
        }
    }
}

/// A parsed view of one dependency entry's git source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    url: RepositoryUrl,
    reference: GitReference,
}

impl GitSource {
    pub fn new(url: RepositoryUrl, reference: GitReference) -> Self {
        Self { url, reference }
    }

    pub fn url(&self) -> &RepositoryUrl {
        &self.url
    }

    pub fn reference(&self) -> &GitReference {
        &self.reference
    }
}

/// How a git dependency names the commit it follows. Mirrors the reference
/// query of Cargo's git source grammar (`?branch=`, `?tag=`, `?rev=`, or no
/// query for the remote default branch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitReference {
    /// No branch, tag, or rev declared: the remote's default branch.
    DefaultBranch,
    Branch(BranchName),
    Tag(String),
    Revision(CommitIdentifier),
}
