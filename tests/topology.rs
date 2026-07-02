//! Witnesses for topology discovery and the ascent order.

#[test]
#[ignore = "scaffold: implement with DependencyGraph::discover on fixture manifests"]
fn edges_come_only_from_manifests_matched_by_repository_identity() {
    // A git dependency whose package name differs from its repository name
    // (nota from nota-next) must match by URL; a dependency outside the
    // configured component set must produce no edge.
    todo!()
}

#[test]
#[ignore = "scaffold: implement with DependencyGraph::ascent_levels"]
fn ascent_levels_put_leaves_first_and_reject_cycles() {
    todo!()
}
