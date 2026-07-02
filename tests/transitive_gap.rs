//! Regression witness for the transitive-lock fallback's package identity.
//!
//! A consumer pins a producer under the repo/table key `nota-next`, with no
//! `package =` rename, and its lock still records the crate name from before
//! the producer dropped `-next` (the lock entry is `nota-next`). At the target
//! revision the producer publishes the package `nota`. The typed lock repin
//! cannot answer for the renamed crate's dependency set, so the controlled
//! `cargo update --precise` fallback fires.
//!
//! The fallback must be invoked with the producer's Cargo *package name*
//! (`nota`) — the identity `cargo update -p` addresses — never the repo/table
//! key (`nota-next`). Passing the key yields `error: no matching package named
//! nota-next` and leaves the invalid typed-edited lock committed. This witness
//! drives the real driver through a resolver that mirrors cargo's spec matching
//! (a spec naming no published package fails exactly as cargo does) and asserts
//! both the package handed to the fallback and the refreshed lock that lands.

mod fixtures;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use fixtures::{
    FixtureBuilderHost, FixtureOpener, FixturePrefetch, FixtureRepository, FixtureVerifierSource,
    revision, standard_config,
};
use synchronizer::configuration::{Component, ComponentCheckout};
use synchronizer::driver::{RunBoundaries, SynchronizerRun};
use synchronizer::error::Error;
use synchronizer::report::Action;
use synchronizer::transitive_lock::{TransitiveLockRequest, TransitiveLockResolver};
use synchronizer::types::{BuilderHost, ComponentName, TomlText};

/// A fallback resolver that mirrors `cargo update -p <spec>`: it refreshes the
/// lock only when the requested package is one the producer actually publishes,
/// and otherwise fails exactly as cargo does for a spec that names nothing. It
/// records every package identity it was asked to update.
struct SpecMatchingResolver {
    published: BTreeSet<&'static str>,
    refreshed_lock: String,
    requested: Rc<RefCell<Vec<String>>>,
}

impl TransitiveLockResolver for SpecMatchingResolver {
    fn resolve_lock(&self, request: &TransitiveLockRequest) -> Result<TomlText, Error> {
        self.requested
            .borrow_mut()
            .push(request.package.as_str().to_string());
        if !self.published.contains(request.package.as_str()) {
            return Err(Error::TransitiveLockResolution {
                component: request.consumer.clone(),
                detail: format!("no matching package named {}", request.package.as_str()),
            });
        }
        Ok(TomlText::new(self.refreshed_lock.clone()))
    }
}

/// The producer: a repository named `nota-next` whose root crate is `nota`
/// (the `-next` was dropped from the crate names). It carries a fresh
/// dependency that a stale consumer lock will not have recorded.
fn nota_next_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"nota\"\n",
                "version = \"0.6.0\"\n",
                "\n",
                "[dependencies]\n",
                "rkyv = \"0.8\"\n",
            )
            .to_string(),
        ),
        (
            "Cargo.lock".to_string(),
            concat!(
                "version = 4\n",
                "\n",
                "[[package]]\n",
                "name = \"nota\"\n",
                "version = \"0.6.0\"\n",
            )
            .to_string(),
        ),
    ])
}

/// The consumer: pins `nota-next` under the repo/table key, with no
/// `package =` rename, and locks it under the pre-rename crate name.
fn consumer_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            concat!(
                "[package]\n",
                "name = \"consumer\"\n",
                "version = \"0.1.0\"\n",
                "\n",
                "[dependencies]\n",
                "nota-next = { git = \"https://github.com/LiGoldragon/nota-next.git\", branch = \"main\" }\n",
            )
            .to_string(),
        ),
        (
            "Cargo.lock".to_string(),
            format!(
                concat!(
                    "version = 4\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"consumer\"\n",
                    "version = \"0.1.0\"\n",
                    "dependencies = [\n \"nota-next\",\n]\n",
                    "\n",
                    "[[package]]\n",
                    "name = \"nota-next\"\n",
                    "version = \"0.5.1\"\n",
                    "source = \"git+https://github.com/LiGoldragon/nota-next.git?branch=main#{nota_old}\"\n",
                ),
                nota_old = revision("nota-old").as_str()
            ),
        ),
    ])
}

/// The valid lock cargo would produce once handed the right package identity:
/// the entry is now the real crate name `nota`, pinned to the new revision.
fn refreshed_consumer_lock() -> String {
    format!(
        concat!(
            "version = 4\n",
            "\n",
            "[[package]]\n",
            "name = \"consumer\"\n",
            "version = \"0.1.0\"\n",
            "dependencies = [\n \"nota\",\n]\n",
            "\n",
            "[[package]]\n",
            "name = \"nota\"\n",
            "version = \"0.6.0\"\n",
            "source = \"git+https://github.com/LiGoldragon/nota-next.git?branch=main#{nota_new}\"\n",
        ),
        nota_new = revision("nota-new").as_str()
    )
}

#[test]
fn the_fallback_updates_the_producer_package_not_the_repo_key() {
    let nota_next = Rc::new(FixtureRepository::new(
        "nota-next",
        revision("nota-new"),
        nota_next_files(),
    ));
    let consumer = Rc::new(FixtureRepository::new(
        "consumer",
        revision("consumer-main"),
        consumer_files(),
    ));
    let opener = FixtureOpener {
        repositories: BTreeMap::from([
            (ComponentName::new("nota-next"), Rc::clone(&nota_next)),
            (ComponentName::new("consumer"), Rc::clone(&consumer)),
        ]),
    };
    let verified = Rc::new(RefCell::new(Vec::new()));
    let requested = Rc::new(RefCell::new(Vec::new()));
    let config = standard_config(vec![
        Component::new(ComponentName::new("nota-next"), ComponentCheckout::AtRoot),
        Component::new(ComponentName::new("consumer"), ComponentCheckout::AtRoot),
    ]);
    let run = SynchronizerRun::with_boundaries(
        config,
        RunBoundaries {
            repository_opener: Box::new(opener),
            nar_hash_source: Box::new(FixturePrefetch),
            builder_host_resolver: Box::new(FixtureBuilderHost {
                host: BuilderHost::new("prometheus"),
            }),
            verifier_source: Box::new(FixtureVerifierSource {
                verified: Rc::clone(&verified),
            }),
            lock_resolver: Box::new(SpecMatchingResolver {
                published: BTreeSet::from(["nota"]),
                refreshed_lock: refreshed_consumer_lock(),
                requested: Rc::clone(&requested),
            }),
        },
    );
    let report = run.execute().expect("the run completes");

    // The fallback was invoked once, with the producer's package identity
    // (`nota`) — the `-p` spec cargo can resolve — never the repo/table key.
    assert_eq!(
        requested.borrow().as_slice(),
        &["nota".to_string()],
        "the fallback must address the producer package name, not the repo/table key",
    );

    // The spec matched, so the fallback refreshed the lock and no failure was
    // collected: the repo-key spec would have failed `no matching package`.
    assert!(
        !report.has_failures(),
        "the fallback resolves cleanly with the right package identity: {:?}",
        report.failures(),
    );

    // The committed consumer lock is the refreshed, valid lock — the entry is
    // the real crate name `nota` at the new revision, not the invalid
    // typed-edited `nota-next` entry.
    let consumer_tip = consumer
        .pushed
        .borrow()
        .last()
        .cloned()
        .expect("the consumer bump was pushed");
    let committed_lock = consumer
        .file_text(&consumer_tip, "Cargo.lock")
        .expect("the bump commit carries the lock");
    assert!(
        committed_lock.contains("name = \"nota\"")
            && committed_lock.contains(&format!("#{}", revision("nota-new").as_str())),
        "the committed lock is the refreshed valid lock:\n{committed_lock}",
    );
    assert!(
        !committed_lock.contains("name = \"nota-next\""),
        "the invalid pre-rename entry must not survive:\n{committed_lock}",
    );

    let consumer_outcome = report
        .levels()
        .iter()
        .flat_map(|level| level.repositories())
        .find(|outcome| outcome.component() == &ComponentName::new("consumer"))
        .expect("the consumer joins the ascent");
    assert!(
        matches!(consumer_outcome.action(), Action::Bumped(_)),
        "the consumer bumps: {consumer_outcome:?}",
    );
}
