//! Meta-repo version propagation.
//!
//! When a low dependency's `main` advances, wire contracts drift and
//! consumers fail to decode each other. The synchronizer cascades the
//! bumps: it discovers the dependency DAG from the manifests, computes
//! what is stale, and ascends the tree from the leaves — editing both pin
//! layers (Cargo and flake) as typed data, committing and pushing each
//! repo's tool-owned `synchronizer` branch (never `main`), build-verifying
//! each bump on a role-resolved builder, collecting every failure, and
//! reporting the run as one NOTA document.
//!
//! Design doc: `ARCHITECTURE.md`. This crate is design + scaffold; bodies
//! are unimplemented pending psyche sign-off.

pub mod build_verify;
pub mod cargo_lock;
pub mod cargo_manifest;
pub mod component_manifests;
pub mod configuration;
pub mod driver;
pub mod error;
pub mod flake_lock;
pub mod flake_manifest;
pub mod git_repository;
pub mod report;
pub mod role_resolution;
pub mod toml_pretty;
pub mod topology;
pub mod types;
pub mod version_resolver;

pub use error::Error;
