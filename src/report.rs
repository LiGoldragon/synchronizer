//! The NOTA run report.
//!
//! One report per run: bumps applied, branches pushed, per-level verify
//! results, and every collected failure — together at the end, per the
//! keep-going rule. Encoding goes through the canonical NOTA codec only.
//!
//! Schema (strict positional; the root record is an untagged struct per
//! the canonical codec — the `SynchronizerReport` label in ARCHITECTURE.md
//! §10 is schema documentation, not a wire tag):
//!
//! ```nota
//! (<started-at> <finished-at> [<level-outcome>] [<failure>])
//! ```

use nota::{NotaDecode, NotaEncode, NotaSource};

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
    pub fn new(
        started_at: Timestamp,
        finished_at: Timestamp,
        levels: Vec<LevelOutcome>,
        failures: Vec<Failure>,
    ) -> Self {
        Self {
            started_at,
            finished_at,
            levels,
            failures,
        }
    }

    /// Render the report as canonical NOTA text.
    pub fn to_nota_text(&self) -> Result<String, Error> {
        Ok(format!("{}\n", self.to_nota()))
    }

    /// Decode a report from canonical NOTA text (round-trip witness
    /// surface).
    pub fn from_nota_text(text: &str) -> Result<Self, Error> {
        NotaSource::new(text)
            .parse::<Self>()
            .map_err(|error| Error::ConfigurationDecode {
                detail: error.to_string(),
            })
    }

    pub fn levels(&self) -> &[LevelOutcome] {
        &self.levels
    }

    pub fn failures(&self) -> &[Failure] {
        &self.failures
    }

    /// Whether the run collected any failure — drives the process exit
    /// code.
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

/// Every repository outcome of one topological level.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct LevelOutcome {
    index: u32,
    repositories: Vec<RepositoryOutcome>,
}

impl LevelOutcome {
    pub fn new(index: u32, repositories: Vec<RepositoryOutcome>) -> Self {
        Self {
            index,
            repositories,
        }
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn repositories(&self) -> &[RepositoryOutcome] {
        &self.repositories
    }
}

/// What happened to one repository this run.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct RepositoryOutcome {
    component: ComponentName,
    action: Action,
    verification: Verification,
}

impl RepositoryOutcome {
    pub fn new(component: ComponentName, action: Action, verification: Verification) -> Self {
        Self {
            component,
            action,
            verification,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn action(&self) -> &Action {
        &self.action
    }

    pub fn verification(&self) -> &Verification {
        &self.verification
    }
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

impl BumpRecord {
    pub fn new(applied: Vec<AppliedBump>, pushed: PushedBranch) -> Self {
        Self { applied, pushed }
    }

    pub fn applied(&self) -> &[AppliedBump] {
        &self.applied
    }

    pub fn pushed(&self) -> &PushedBranch {
        &self.pushed
    }
}

/// One pin movement in one layer.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct AppliedBump {
    dependency: ComponentName,
    layer: PinLayer,
    previous: PinValue,
    next: PinValue,
}

impl AppliedBump {
    pub fn new(
        dependency: ComponentName,
        layer: PinLayer,
        previous: PinValue,
        next: PinValue,
    ) -> Self {
        Self {
            dependency,
            layer,
            previous,
            next,
        }
    }

    pub fn dependency(&self) -> &ComponentName {
        &self.dependency
    }

    pub fn layer(&self) -> PinLayer {
        self.layer
    }

    pub fn previous(&self) -> &PinValue {
        &self.previous
    }

    pub fn next(&self) -> &PinValue {
        &self.next
    }
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

impl PushedBranch {
    pub fn new(branch: BranchName, tip: CommitIdentifier) -> Self {
        Self { branch, tip }
    }

    pub fn branch(&self) -> &BranchName {
        &self.branch
    }

    pub fn tip(&self) -> &CommitIdentifier {
        &self.tip
    }
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

impl Failure {
    pub fn new(component: ComponentName, stage: FailureStage, detail: FailureDetail) -> Self {
        Self {
            component,
            stage,
            detail,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn stage(&self) -> FailureStage {
        self.stage
    }

    pub fn detail(&self) -> &FailureDetail {
        &self.detail
    }
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
/// excerpt. Carried as NOTA text in the rendered report.
#[derive(Debug, Clone, PartialEq, Eq, NotaDecode, NotaEncode)]
pub struct FailureDetail(String);

impl FailureDetail {
    pub fn new(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
