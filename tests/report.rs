//! Witnesses for the NOTA report schema.

use synchronizer::report::{
    Action, AppliedBump, BumpRecord, Failure, FailureDetail, FailureStage, LevelOutcome, PinValue,
    PushedBranch, RepositoryOutcome, SynchronizerReport, Verification,
};
use synchronizer::topology::PinLayer;
use synchronizer::types::{BranchName, BuilderHost, CommitIdentifier, ComponentName, Timestamp};

fn example_report() -> SynchronizerReport {
    let frame = ComponentName::new("signal-frame");
    let router = ComponentName::new("signal-router");
    let old = CommitIdentifier::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let new = CommitIdentifier::new("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let tip = CommitIdentifier::new("cccccccccccccccccccccccccccccccccccccccc");
    let host = BuilderHost::new("prometheus");
    SynchronizerReport::new(
        Timestamp::from_unix_seconds(1_750_000_000),
        Timestamp::from_unix_seconds(1_750_000_060),
        vec![
            LevelOutcome::new(
                0,
                vec![RepositoryOutcome::new(
                    frame.clone(),
                    Action::AlreadyAligned,
                    Verification::NotAttempted,
                )],
            ),
            LevelOutcome::new(
                1,
                vec![RepositoryOutcome::new(
                    router.clone(),
                    Action::Bumped(BumpRecord::new(
                        vec![
                            AppliedBump::new(
                                frame.clone(),
                                PinLayer::CargoLock,
                                PinValue::Revision(old.clone()),
                                PinValue::Revision(new.clone()),
                            ),
                            AppliedBump::new(
                                frame.clone(),
                                PinLayer::CargoManifest,
                                PinValue::Reference(BranchName::main()),
                                PinValue::Reference(BranchName::synchronizer()),
                            ),
                        ],
                        PushedBranch::new(BranchName::synchronizer(), tip.clone()),
                    )),
                    Verification::VerifyFailed(host),
                )],
            ),
        ],
        vec![Failure::new(
            router,
            FailureStage::Verify,
            FailureDetail::new(
                "error: check router-daemon-answers-sockets failed with exit code 101",
            ),
        )],
    )
}

#[test]
fn report_round_trips_through_the_canonical_codec() {
    let report = example_report();
    let encoded = report.to_nota_text().expect("the report encodes");
    let decoded = SynchronizerReport::from_nota_text(&encoded).expect("the report decodes");
    assert_eq!(decoded, report);
}

#[test]
fn failures_drive_the_exit_signal() {
    let report = example_report();
    assert!(report.has_failures());
    let clean = SynchronizerReport::new(
        Timestamp::from_unix_seconds(1),
        Timestamp::from_unix_seconds(2),
        Vec::new(),
        Vec::new(),
    );
    assert!(!clean.has_failures());
}
