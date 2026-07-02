//! Typed model of a component's `Cargo.lock`.
//!
//! Reading goes through serde and the TOML deserializer; the `source` pin
//! is parsed by a winnow grammar over the `git+<url>?<query>#<rev>` shape —
//! never ad hoc splitting. Writing goes through the format-preserving
//! document, so a repin changes exactly the `version` and `source` values
//! of the affected entries and leaves Cargo's own rendering (header line
//! included) byte-identical everywhere else.

use serde::Deserialize;
use winnow::Parser;
use winnow::combinator::opt;
use winnow::token::take_while;

use crate::cargo_manifest::{DependencyName, GitReference, PackageVersion};
use crate::error::Error;
use crate::toml_document::PreservedTomlDocument;
use crate::topology::PinLayer;
use crate::types::{BranchName, CommitIdentifier, ComponentName, RepositoryUrl, TomlText};

/// A `Cargo.lock` document (lock format versions 3 and 4): the serde read
/// model plus the format-preserving write document.
#[derive(Debug, Clone)]
pub struct CargoLock {
    component: ComponentName,
    read: CargoLockModel,
    document: PreservedTomlDocument,
}

impl CargoLock {
    /// Deserialize a lock from TOML text.
    pub fn from_toml_text(text: &str, component: &ComponentName) -> Result<Self, Error> {
        let read: CargoLockModel = toml::from_str(text).map_err(|error| Error::ManifestDecode {
            component: component.clone(),
            layer: PinLayer::CargoLock,
            detail: error.to_string(),
        })?;
        let document = PreservedTomlDocument::parse(text, component, PinLayer::CargoLock)?;
        Ok(Self {
            component: component.clone(),
            read,
            document,
        })
    }

    /// Every git-sourced package entry, with its parsed pin.
    pub fn git_packages(&self) -> Result<Vec<(DependencyName, GitPin)>, Error> {
        let mut packages = Vec::new();
        for entry in &self.read.packages {
            let Some(source) = &entry.source else {
                continue;
            };
            if let Some(pin) = source.git_pin()? {
                packages.push((entry.name.clone(), pin));
            }
        }
        Ok(packages)
    }

    /// The recorded version of the named package, when it is present.
    pub fn package_version(&self, name: &DependencyName) -> Option<&PackageVersion> {
        self.read
            .packages
            .iter()
            .find(|entry| &entry.name == name)
            .map(|entry| &entry.version)
    }

    /// The dependency package names the lock records for `name` — the
    /// gap-detection read: when the dependency's own manifest at the target
    /// revision declares a package the lock does not record here, the typed
    /// repin cannot complete the graph and the controlled
    /// `cargo update --precise` fallback takes over.
    pub fn recorded_dependencies_of(&self, name: &DependencyName) -> Option<Vec<DependencyName>> {
        self.read
            .packages
            .iter()
            .find(|entry| &entry.name == name)
            .map(|entry| {
                entry
                    .dependencies
                    .iter()
                    .map(|dependency| {
                        // Entries disambiguate as "name" or "name <version>".
                        let name = dependency
                            .split_whitespace()
                            .next()
                            .unwrap_or(dependency.as_str());
                        DependencyName::new(name)
                    })
                    .collect()
            })
    }

    /// Repin the named git package in both the read model and the
    /// format-preserving document, returning the previous locked revision.
    ///
    /// Sets the locked revision, rewrites the source reference query
    /// (`?branch=...`) to `reference`, and synchronizes the recorded package
    /// version to `version_at_target` — the version the dependency's own
    /// manifest declares at the target revision.
    ///
    /// This covers rev-only drift. If the dependency's *own dependency set*
    /// changed at the target revision, the typed edit cannot invent the new
    /// transitive entries; gap detection routes that case to the controlled
    /// `cargo update --precise` fallback, and build-verify catches whatever
    /// remains (ARCHITECTURE.md §4).
    pub fn repin_git_package(
        &mut self,
        name: &DependencyName,
        reference: GitReference,
        revision: CommitIdentifier,
        version_at_target: Option<PackageVersion>,
    ) -> Result<CommitIdentifier, Error> {
        let entry = self
            .read
            .packages
            .iter_mut()
            .find(|entry| {
                &entry.name == name
                    && entry
                        .source
                        .as_ref()
                        .is_some_and(|source| matches!(source.git_pin(), Ok(Some(_))))
            })
            .ok_or_else(|| Error::NotComponentDependency {
                consumer: self.component.clone(),
                dependency: name.as_str().to_string(),
            })?;
        let source = entry.source.as_ref().expect("git source checked");
        let pin = source.git_pin()?.expect("git pin checked");
        let previous = pin.revision.clone();
        let next_pin = GitPin {
            url: pin.url.clone(),
            reference,
            revision,
        };
        let next_source = next_pin.to_source_text();
        let next_version = version_at_target.unwrap_or_else(|| entry.version.clone());
        entry.version = next_version.clone();
        entry.source = Some(next_source.clone());
        self.rewrite_document_entry(name, &next_version, &next_source)?;
        Ok(previous)
    }

    /// The document text with all edits applied, byte-identical to Cargo's
    /// own rendering everywhere untouched.
    pub fn to_toml_text(&self) -> TomlText {
        self.document.to_toml_text()
    }

    fn rewrite_document_entry(
        &mut self,
        name: &DependencyName,
        version: &PackageVersion,
        source: &SourceText,
    ) -> Result<(), Error> {
        let document = self.document.as_document_mut();
        let packages = document
            .get_mut("package")
            .and_then(toml_edit::Item::as_array_of_tables_mut)
            .ok_or_else(|| Error::ManifestEncode {
                component: self.component.clone(),
                layer: PinLayer::CargoLock,
                detail: "lock document holds no [[package]] array".to_string(),
            })?;
        for entry in packages.iter_mut() {
            let matches_name = entry
                .get("name")
                .and_then(toml_edit::Item::as_str)
                .is_some_and(|entry_name| entry_name == name.as_str());
            let is_git = entry
                .get("source")
                .and_then(toml_edit::Item::as_str)
                .is_some_and(|text| text.starts_with("git+"));
            if matches_name && is_git {
                entry.insert("version", toml_edit::value(version.as_str()));
                entry.insert("source", toml_edit::value(source.as_str()));
                return Ok(());
            }
        }
        Err(Error::ManifestEncode {
            component: self.component.clone(),
            layer: PinLayer::CargoLock,
            detail: format!("no git-sourced [[package]] entry named {}", name.as_str()),
        })
    }
}

/// The serde read model of a lock.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct CargoLockModel {
    #[allow(dead_code)]
    version: u32,
    #[serde(default, rename = "package")]
    packages: Vec<LockedPackage>,
}

/// One `[[package]]` entry. External field names preserved at the serde
/// boundary; entries without a `source` are path/workspace members.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct LockedPackage {
    name: DependencyName,
    version: PackageVersion,
    #[serde(default)]
    source: Option<SourceText>,
    #[serde(default)]
    dependencies: Vec<String>,
}

/// The raw `source` field text, e.g.
/// `git+https://github.com/LiGoldragon/signal-frame.git?branch=main#<rev>`.
/// Parsed into [`GitPin`] by a winnow grammar, never split ad hoc.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SourceText(String);

impl SourceText {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse a git source into its typed pin, or `None` for registry and
    /// other non-git sources.
    pub fn git_pin(&self) -> Result<Option<GitPin>, Error> {
        if !self.0.starts_with("git+") {
            return Ok(None);
        }
        let mut input = self.0.as_str();
        let pin = GitPin::parse(&mut input).map_err(|_| Error::RepositoryUrlUnparseable {
            url: self.0.clone(),
        })?;
        Ok(Some(pin))
    }
}

/// A parsed git lock pin: where the package comes from and the exact commit
/// it is locked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPin {
    url: RepositoryUrl,
    reference: GitReference,
    revision: CommitIdentifier,
}

impl GitPin {
    pub fn new(url: RepositoryUrl, reference: GitReference, revision: CommitIdentifier) -> Self {
        Self {
            url,
            reference,
            revision,
        }
    }

    pub fn url(&self) -> &RepositoryUrl {
        &self.url
    }

    pub fn reference(&self) -> &GitReference {
        &self.reference
    }

    pub fn revision(&self) -> &CommitIdentifier {
        &self.revision
    }

    /// Winnow grammar over Cargo's git source shape:
    /// `git+<url>[?branch=<name>|?tag=<name>|?rev=<rev>]#<revision>`.
    fn parse(input: &mut &str) -> winnow::Result<Self> {
        let _ = "git+".parse_next(input)?;
        let url = take_while(1.., |character: char| character != '?' && character != '#')
            .parse_next(input)?;
        let query = opt((
            '?',
            take_while(1.., |character: char| character != '=' && character != '#'),
            '=',
            take_while(1.., |character: char| character != '#'),
        ))
        .parse_next(input)?;
        let _ = '#'.parse_next(input)?;
        let revision =
            take_while(1.., |character: char| character.is_ascii_hexdigit()).parse_next(input)?;
        let reference = match query {
            None => GitReference::DefaultBranch,
            Some((_, key, _, value)) => match key {
                "branch" => GitReference::Branch(BranchName::new(value)),
                "tag" => GitReference::Tag(value.to_string()),
                "rev" => GitReference::Revision(CommitIdentifier::new(value)),
                _ => {
                    return Err(winnow::error::ContextError::new());
                }
            },
        };
        Ok(Self {
            url: RepositoryUrl::new(url),
            reference,
            revision: CommitIdentifier::new(revision),
        })
    }

    /// Render the pin back into Cargo's source text shape.
    pub fn to_source_text(&self) -> SourceText {
        let query = match &self.reference {
            GitReference::DefaultBranch => String::new(),
            GitReference::Branch(branch) => format!("?branch={}", branch.as_str()),
            GitReference::Tag(tag) => format!("?tag={tag}"),
            GitReference::Revision(revision) => format!("?rev={}", revision.as_str()),
        };
        SourceText::new(format!(
            "git+{}{}#{}",
            self.url.as_str(),
            query,
            self.revision.as_str()
        ))
    }
}
