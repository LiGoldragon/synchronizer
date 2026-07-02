//! Dependency topology, discovered — never declared.
//!
//! The configuration names the participating components; the edges between
//! them come only from the manifests: Cargo git dependencies matched by
//! repository URL, and flake GitHub inputs matched by owner and repository.
//! The result must be a DAG; a cycle is run-fatal because it admits no
//! topological ascent.

use nota::{NotaDecode, NotaEncode};

use crate::component_manifests::ComponentManifests;
use crate::error::Error;
use crate::types::ComponentName;

/// Which pin surface an edge was discovered in (and therefore where its
/// bump must be written).
#[derive(Debug, Clone, Copy, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum PinLayer {
    CargoManifest,
    CargoLock,
    FlakeManifest,
    FlakeLock,
}

/// One consumer-to-producer pin discovered in the consumer's manifests.
///
/// A consumer usually holds several edges to the same producer — one per
/// layer that pins it (`CargoLock` and `FlakeLock` for a typical sibling
/// dependency). Each layer bumps independently and is reported
/// independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEdge {
    consumer: ComponentName,
    producer: ComponentName,
    layer: PinLayer,
}

impl DependencyEdge {
    pub fn consumer(&self) -> &ComponentName {
        &self.consumer
    }

    pub fn producer(&self) -> &ComponentName {
        &self.producer
    }

    pub fn layer(&self) -> PinLayer {
        self.layer
    }
}

/// The dependency DAG over the configured components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraph {
    components: Vec<ComponentName>,
    edges: Vec<DependencyEdge>,
}

impl DependencyGraph {
    /// Discover every edge from the loaded manifests. A git dependency or
    /// flake input pointing outside the configured component set is not an
    /// edge (third-party inputs are out of scope by design).
    pub fn discover(manifests: &[ComponentManifests]) -> Result<Self, Error> {
        todo!()
    }

    /// All edges whose consumer is `consumer`.
    pub fn dependencies_of(&self, consumer: &ComponentName) -> Vec<&DependencyEdge> {
        todo!()
    }

    /// The ascent order: level 0 holds the leaves (components with no
    /// component dependencies), level N holds components all of whose
    /// dependencies sit in levels below N. `Err(DependencyCycle)` when the
    /// graph is not a DAG.
    pub fn ascent_levels(&self) -> Result<TopologicalLevels, Error> {
        todo!("Kahn's algorithm; deterministic order within a level (by name)")
    }
}

/// Components grouped by topological level, leaves first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologicalLevels(Vec<Vec<ComponentName>>);

impl TopologicalLevels {
    pub fn levels(&self) -> &[Vec<ComponentName>] {
        &self.0
    }
}
