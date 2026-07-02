//! Witnesses for role→host resolution against the cluster proposal
//! surface.
//!
//! The fixture mirrors the frozen horizon-rs shapes: a 5-field
//! `ClusterProposal` root and 17-field `NodeProposal` records whose
//! trailing fields are `online` and `services`. No hostname exists in the
//! tool; the selected node name comes entirely from this cluster data.

use synchronizer::role_resolution::ClusterRoleView;
use synchronizer::types::BuilderRole;

/// Three nodes holding NixBuilder: an offline node with the largest
/// declared capacity, an online node with capacity 6, and an online node
/// with default capacity. The proposal's own capacity and online data must
/// decide.
fn fixture_proposal() -> String {
    let node = |species: &str, online: &str, services: &str| {
        format!(
            "({species}\n  Max\n  Max\n  (Metal None 4 None None None None None None None [])\n  \
             (Qwerty Uefi {{ / (/dev/vda Ext4 []) }} [])\n  (AAAAfixture None None)\n  []\n  None\n  \
             None\n  False\n  False\n  []\n  False\n  False\n  None\n  {online}\n  {services})"
        )
    };
    format!(
        "({{\n  heavy-offline {}\n  prometheus {}\n  ouranos {}\n  zeus {}\n}}\n{{}}\n{{}}\n(Max {{}} {{}} {{}})\n(criome [example.net]))",
        node(
            "LargeAiRouter",
            "(Some False)",
            "[(TailnetClient) (NixBuilder (Some 32))]"
        ),
        node(
            "LargeAiRouter",
            "(Some True)",
            "[(TailnetClient) (NixBuilder (Some 6)) (NixCache)]"
        ),
        node("EdgeTesting", "None", "[(TailnetClient) (NixBuilder None)]"),
        node("Edge", "(Some True)", "[(TailnetClient)]"),
    )
}

#[test]
fn builder_role_resolves_to_the_highest_capacity_online_node() {
    let view = ClusterRoleView::from_nota_text(&fixture_proposal())
        .expect("the fixture proposal decodes through the narrow view");
    let host = view
        .host_for(&BuilderRole::new("NixBuilder"))
        .expect("an online NixBuilder exists");
    assert_eq!(
        host.as_str(),
        "prometheus",
        "the offline 32-job node is out; capacity 6 beats the default 1"
    );
}

#[test]
fn unheld_roles_fail_loud() {
    let view =
        ClusterRoleView::from_nota_text(&fixture_proposal()).expect("the fixture proposal decodes");
    assert!(
        view.host_for(&BuilderRole::new("TailnetController"))
            .is_err()
    );
}

#[test]
fn schema_drift_fails_loud_instead_of_guessing_positions() {
    // A 16-field node record: the horizon NodeProposal schema moved and the
    // narrow view must refuse, not misread.
    let drifted = "({\n  atlas (Edge Max Max (Metal None 4 None None None None None None None []) \
                   (Qwerty Uefi { / (/dev/vda Ext4 []) } []) (AAAAfixture None None) [] None None \
                   False False [] False False None [(NixBuilder None)])\n} {} {} (Max {} {} {}) \
                   (criome [example.net]))";
    assert!(ClusterRoleView::from_nota_text(drifted).is_err());
}
