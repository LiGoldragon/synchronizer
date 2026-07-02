//! Role-indirect builder lookup.
//!
//! The tool knows a CriomOS *role* name from configuration; it never holds
//! a hostname. Resolution goes through this boundary so no hostname can
//! appear in source or configuration (invariant, ARCHITECTURE.md §13).
//!
//! The confirmed authoritative surface (OS-ops discovery, §14 q5) is the
//! cluster proposal document: the horizon-rs `ClusterProposal` NOTA datom
//! whose per-node `services` vectors author every cluster role
//! (`(NixBuilder (Some 6))` and siblings). Cluster flakes expose no
//! role→host output, the production cluster repository is not a flake, and
//! Lojix records deployment generations, not roles. horizon-rs owns the
//! full schema; this module holds a *narrow positional view* — decoded
//! through the canonical codec primitives, count-strict against the frozen
//! proposal shape (5 root fields, 17 node fields) — that reads exactly the
//! node names, online states, and service vectors. A count mismatch means
//! the horizon schema moved and resolution fails loud (collected as a
//! RoleResolution failure; bumps and pushes still proceed, §9).

use nota::{Delimiter, NotaBlock, NotaDecode, NotaDecodeError, NotaSource};

use crate::configuration::ClusterConfiguration;
use crate::error::Error;
use crate::types::{BuilderHost, BuilderRole};

/// The role-to-host boundary.
pub trait ClusterRoleDirectory {
    /// Resolve `role` to the host currently holding it in the cluster.
    fn host_for(&self, role: &BuilderRole) -> Result<BuilderHost, Error>;
}

/// The production directory: reads role assignments from the configured
/// cluster proposal document.
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
        let ClusterConfiguration::ClusterProposal(path) = &self.cluster;
        let text = std::fs::read_to_string(path.as_path_buffer()).map_err(|error| {
            Error::RoleUnresolved {
                role: role.clone(),
                detail: format!("cluster proposal unreadable: {error}"),
            }
        })?;
        let view =
            ClusterRoleView::from_nota_text(&text).map_err(|error| Error::RoleUnresolved {
                role: role.clone(),
                detail: format!("cluster proposal undecodable: {error}"),
            })?;
        view.host_for(role)
    }
}

/// The narrow role view of a cluster proposal: every node with its online
/// state and authored services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterRoleView {
    nodes: Vec<NodeRoleView>,
}

impl ClusterRoleView {
    pub fn from_nota_text(text: &str) -> Result<Self, NotaDecodeError> {
        NotaSource::new(text).parse::<Self>()
    }

    pub fn nodes(&self) -> &[NodeRoleView] {
        &self.nodes
    }

    /// The cluster-authored host holding `role`: among the online nodes
    /// whose services carry the role, the one with the greatest declared
    /// capacity (`maximum_jobs`, absent meaning one job at a time), name
    /// order breaking ties. The selection reads only cluster data — no
    /// hostname or preference lives in the tool.
    pub fn host_for(&self, role: &BuilderRole) -> Result<BuilderHost, Error> {
        self.nodes
            .iter()
            .filter(|node| node.is_online() && node.holds_role(role))
            .max_by(|left, right| {
                left.capacity_for(role)
                    .cmp(&right.capacity_for(role))
                    // Prefer the *smaller* name on equal capacity: reversed
                    // in max_by so the lexicographically first node wins.
                    .then_with(|| right.name.cmp(&left.name))
            })
            .map(|node| BuilderHost::new(node.name.clone()))
            .ok_or_else(|| Error::RoleUnresolved {
                role: role.clone(),
                detail: "no online node in the cluster proposal holds the role".to_string(),
            })
    }
}

/// The frozen positional shape of the horizon-rs `ClusterProposal` root:
/// nodes, users, domains, trust, domain-configuration.
const CLUSTER_PROPOSAL_FIELD_COUNT: usize = 5;

/// The frozen positional shape of the horizon-rs `NodeProposal` record.
/// The datom schema is count-strict and moves in lockstep with its
/// consumers; this view checks the count and fails loud on drift.
const NODE_PROPOSAL_FIELD_COUNT: usize = 17;

/// The `online` field position inside a node record.
const NODE_ONLINE_POSITION: usize = 15;

/// The `services` field position inside a node record.
const NODE_SERVICES_POSITION: usize = 16;

impl NotaDecode for ClusterRoleView {
    fn from_nota_block(block: &nota::Block) -> Result<Self, NotaDecodeError> {
        let children = NotaBlock::new(block).expect_children(
            Delimiter::Parenthesis,
            "ClusterProposal",
            CLUSTER_PROPOSAL_FIELD_COUNT,
        )?;
        let node_entries = NotaBlock::new(&children[0])
            .expect_delimited(Delimiter::Brace, "ClusterProposal nodes")?;
        if node_entries.len() % 2 != 0 {
            return Err(NotaDecodeError::InvalidValue {
                type_name: "ClusterProposal nodes",
                value: format!("{} objects", node_entries.len()),
                reason: "a map holds key/value pairs".to_string(),
            });
        }
        let mut nodes = Vec::new();
        for pair in node_entries.chunks_exact(2) {
            let name = String::from_nota_block(&pair[0])?;
            let node = NodeRoleView::from_named_block(name, &pair[1])?;
            nodes.push(node);
        }
        Ok(Self { nodes })
    }
}

/// One node's role-relevant slice of its `NodeProposal` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRoleView {
    name: String,
    online: Option<bool>,
    services: Vec<ServiceRoleView>,
}

impl NodeRoleView {
    fn from_named_block(name: String, block: &nota::Block) -> Result<Self, NotaDecodeError> {
        let children = NotaBlock::new(block).expect_children(
            Delimiter::Parenthesis,
            "NodeProposal",
            NODE_PROPOSAL_FIELD_COUNT,
        )?;
        let online = Option::<bool>::from_nota_block(&children[NODE_ONLINE_POSITION])?;
        let services = Vec::<ServiceRoleView>::from_nota_block(&children[NODE_SERVICES_POSITION])?;
        Ok(Self {
            name,
            online,
            services,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// `None` defaults to online, matching the proposal schema.
    fn is_online(&self) -> bool {
        self.online.unwrap_or(true)
    }

    fn holds_role(&self, role: &BuilderRole) -> bool {
        self.services
            .iter()
            .any(|service| service.kind == role.as_str())
    }

    /// The declared capacity for `role`; absence means one job at a time,
    /// matching the proposal schema.
    fn capacity_for(&self, role: &BuilderRole) -> u32 {
        self.services
            .iter()
            .find(|service| service.kind == role.as_str())
            .and_then(|service| service.capacity)
            .unwrap_or(1)
    }
}

/// One service record, reduced to its kind atom and optional capacity
/// payload (`(NixBuilder (Some 6))`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRoleView {
    kind: String,
    capacity: Option<u32>,
}

impl NotaDecode for ServiceRoleView {
    fn from_nota_block(block: &nota::Block) -> Result<Self, NotaDecodeError> {
        let children =
            NotaBlock::new(block).expect_delimited(Delimiter::Parenthesis, "NodeService")?;
        let kind = children
            .first()
            .and_then(|child| child.demote_to_string())
            .ok_or(NotaDecodeError::ExpectedAtom {
                type_name: "NodeService kind",
            })?
            .to_string();
        let capacity = match (kind.as_str(), children.get(1)) {
            ("NixBuilder", Some(payload)) => Option::<u32>::from_nota_block(payload)?,
            _ => None,
        };
        Ok(Self { kind, capacity })
    }
}
