//! Witnesses for the NOTA configuration schema.
//!
//! The authoritative wire shape is the Rust schema in
//! `src/configuration.rs` plus these round-trip examples; the pseudo-NOTA
//! in ARCHITECTURE.md §3 is documentation.

#[test]
#[ignore = "scaffold: implement with SynchronizerConfig::from_nota_text"]
fn configuration_round_trips_through_the_canonical_codec() {
    // Decode the ARCHITECTURE.md §3 example document, re-encode it, and
    // assert canonical-text equality.
    todo!()
}

#[test]
#[ignore = "scaffold: implement with SynchronizerConfig::checkout_path"]
fn at_root_checkout_resolves_against_the_checkout_root() {
    todo!()
}
