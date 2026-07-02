//! Role-indirect builder lookup.
//!
//! The tool knows a CriomOS *role* name from configuration; it never holds
//! a hostname. Resolution goes through this boundary so no hostname can
//! appear in source or configuration (invariant, ARCHITECTURE.md §13).

use crate::configuration::ClusterConfiguration;
use crate::error::Error;
use crate::types::{BuilderHost, BuilderRole};

/// The role-to-host boundary.
pub trait ClusterRoleDirectory {
    /// Resolve `role` to the host currently holding it in the cluster.
    fn host_for(&self, role: &BuilderRole) -> Result<BuilderHost, Error>;
}

/// The planned production directory: reads role assignments from the
/// configured CriomOS cluster configuration.
///
/// TODO(implementation, OS-ops): confirm the concrete cluster-config
/// surface to read — candidate surfaces are the cluster flake's node/role
/// outputs (CriomOS-test-cluster shape) and a Lojix query; see
/// ARCHITECTURE.md §8 and §14. The trait boundary is the design; the
/// reader behind it is the implementation detail to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriomosClusterDirectory {
    cluster: ClusterConfiguration,
}

impl CriomosClusterDirectory {
    pub fn new(cluster: ClusterConfiguration) -> Self {
        Self { cluster }
    }
}

impl ClusterRoleDirectory for CriomosClusterDirectory {
    fn host_for(&self, role: &BuilderRole) -> Result<BuilderHost, Error> {
        todo!("evaluate the configured cluster surface; error RoleUnresolved when absent")
    }
}
