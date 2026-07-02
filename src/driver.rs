//! The run driver: self-driving topological ascent from the leaves.
//!
//! The driver owns one run. It composes the boundaries (git, prefetch,
//! role directory, verifier) and walks the algorithm of ARCHITECTURE.md
//! §11: discover topology at pushed truth, compute staleness against
//! resolved targets, bump/commit/push/verify level by level, collect every
//! failure, and render one NOTA report at the end.
//!
//! Only configuration load and topology discovery are run-fatal; every
//! per-repository failure is collected and the ascent continues.

use crate::configuration::SynchronizerConfig;
use crate::error::Error;
use crate::flake_lock::NarHashSource;
use crate::report::SynchronizerReport;
use crate::role_resolution::ClusterRoleDirectory;

/// One synchronizer run.
pub struct SynchronizerRun {
    config: SynchronizerConfig,
    nar_hash_source: Box<dyn NarHashSource>,
    role_directory: Box<dyn ClusterRoleDirectory>,
}

impl SynchronizerRun {
    /// Compose a run from configuration and the production boundaries.
    pub fn new(
        config: SynchronizerConfig,
        nar_hash_source: Box<dyn NarHashSource>,
        role_directory: Box<dyn ClusterRoleDirectory>,
    ) -> Self {
        Self {
            config,
            nar_hash_source,
            role_directory,
        }
    }

    /// Execute the ascent and return the collected report.
    ///
    /// `Err` only for run-fatal conditions (unreadable configuration,
    /// undiscoverable topology, dependency cycle). Everything else —
    /// including every failed bump, push, and verify — lands inside the
    /// report.
    pub fn execute(self) -> Result<SynchronizerReport, Error> {
        todo!(
            "1 open repositories; 2 query remote main tips; 3 fetch + load manifests at tips; \
             4 discover graph + ascent levels; 5 per level, per component: resolve targets, \
             compute stale pins, apply typed bumps, commit on main tip, push synchronizer \
             branch, record in ledger, verify on builder; 6 render report"
        )
    }
}
