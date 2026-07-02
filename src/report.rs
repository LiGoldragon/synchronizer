//! The NOTA run report.
//!
//! One report per run: bumps applied, branches pushed, per-level verify
//! results, and every collected failure — together at the end, per the
//! keep-going rule. Encoding goes through the canonical NOTA codec only.
//!
//! Schema (strict positional; see ARCHITECTURE.md §10):
//!
//! ```nota
//! (SynchronizerReport <started-at> <finished-at> [<level-outcome>] [<failure>])
//! ```

use nota::{NotaDecode, NotaEncode};

use crate::error::Error;
use crate::topology::PinLayer;
use crate::types::{BranchName, BuilderHost, CommitIdentifier, ComponentName, Timestamp};

/// One run's outcome, levels in ascent order.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct SynchronizerReport {
    started_at: Timestamp,
    finished_at: Timestamp,
    levels: Vec<LevelOutcome>,
    failures: Vec<Failure>,
}

impl SynchronizerReport {
    /// Render the report as canonical NOTA text.
    pub fn to_nota_text(&self) -> Result<String, Error> {
        todo!("encode through the canonical nota codec")
    }

    /// Whether the run collected any failure — drives the process exit
    /// code.
    pub fn has_failures(&self) -> bool {
        todo!()
    }
}

/// Every repository outcome of one topological level.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct LevelOutcome {
    index: u32,
    repositories: Vec<RepositoryOutcome>,
}

/// What happened to one repository this run.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct RepositoryOutcome {
    component: ComponentName,
    action: Action,
    verification: Verification,
}

/// The bump action taken for one repository.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum Action {
    /// Every pin already matched its resolved target; nothing was written.
    AlreadyAligned,
    /// Pins were bumped, committed, and pushed.
    Bumped(BumpRecord),
    /// The bump could not complete; detail lives in the failures vector.
    BumpFailed(FailureStage),
}

/// A completed bump: what moved and the branch tip that now carries it.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct BumpRecord {
    applied: Vec<AppliedBump>,
    pushed: PushedBranch,
}

/// One pin movement in one layer.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct AppliedBump {
    dependency: ComponentName,
    layer: PinLayer,
    previous: PinValue,
    next: PinValue,
}

/// A pin value as it appears in a manifest or lock: an exact revision
/// (lock layers) or a branch reference (manifest layers).
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum PinValue {
    Revision(CommitIdentifier),
    Reference(BranchName),
}

/// The pushed synchronizer branch and its tip.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct PushedBranch {
    branch: BranchName,
    tip: CommitIdentifier,
}

/// The verify result for one repository.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum Verification {
    Verified(BuilderHost),
    /// Detail lives in the failures vector.
    VerifyFailed(BuilderHost),
    /// No verify ran: the bump failed earlier, no builder host resolved,
    /// or nothing changed.
    NotAttempted,
}

/// One collected failure. Failures never abort the ascent; they are
/// reported together here.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct Failure {
    component: ComponentName,
    stage: FailureStage,
    detail: FailureDetail,
}

/// Where in the per-repository pipeline a failure happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NotaDecode, NotaEncode)]
pub enum FailureStage {
    Fetch,
    Resolve,
    ManifestEdit,
    LockEdit,
    Prefetch,
    Commit,
    Push,
    RoleResolution,
    Verify,
}

/// Diagnostic text for one failure: a decode error or a command output
/// excerpt. Carried as NOTA pipe text in the rendered report.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct FailureDetail(String);

impl FailureDetail {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
