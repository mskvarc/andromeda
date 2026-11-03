//! A compact node a Context Producer wrote, read without the members it may
//! not supply.
//!
//! Clause 5.2.2 makes `createdAt`, `modifiedAt` and `deletedAt` system
//! generated: "They shall not be provided by Context Producers. In the event
//! that they are provided ... implementations shall ignore them". Clause 5.2.5
//! and its siblings make the `previous...` member of a family read-only in the
//! same way. A request is therefore read as if it never named them, on the
//! entity and on every attribute instance alike, whatever they hold; a value is
//! never filtered inside, since what a node holds as JSON is taken from the
//! node as written.

use crate::{
    compact_node::{CompactNode, NodeMembers, NodeShape},
    value_budget::ValueBudget,
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde::de::DeserializeSeed;

/// The members clause 5.2.2 has the broker generate.
const SYSTEM_GENERATED: [&str; 3] = [compact_properties::CREATED_AT, compact_properties::MODIFIED_AT, compact_properties::DELETED_AT];

/// Whether a `deletedAt` a node names is read.
///
/// A Temporal Evolution a client writes records a deleted instance as the
/// instance whose value is the NGSI-LD Null "and the deletedAt Temporal
/// Property set" (clauses 4.5.7 and 5.5.4), so the write of one is the one
/// place a producer's `deletedAt` is data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletedAt {
    /// The member is ignored, as clause 5.2.2 has every implementation do.
    Ignored,
    /// The member is read as written.
    Read,
}

/// Whether the member `name` is one the broker generates and a Context
/// Producer may not supply: a clause 5.2.2 system time, or the `previous...`
/// member of a family, which clause 5.2.5 and its siblings make read-only and
/// leave for the broker to report.
fn is_broker_generated(name: &str, deleted_at: DeletedAt) -> bool {
    let generated = match deleted_at {
        DeletedAt::Ignored => SYSTEM_GENERATED.contains(&name),
        DeletedAt::Read => name == compact_properties::CREATED_AT || name == compact_properties::MODIFIED_AT,
    };
    generated || AttributeType::ALL.iter().any(|family| family.previous_key() == name)
}

/// A compact node a Context Producer wrote.
#[derive(Debug, Clone, Copy)]
pub struct ProducedNode<N> {
    /// The node as written.
    node: N,
    /// Whether a `deletedAt` is read.
    deleted_at: DeletedAt,
}

impl<N> ProducedNode<N> {
    /// Read `node` as a Context Producer's.
    #[must_use]
    pub const fn new(node: N) -> ProducedNode<N> {
        ProducedNode {
            node,
            deleted_at: DeletedAt::Ignored,
        }
    }

    /// Read `node` as a Context Producer's Temporal Evolution, whose deleted
    /// instances carry their `deletedAt`.
    #[must_use]
    pub const fn recording_deletions(node: N) -> ProducedNode<N> {
        ProducedNode {
            node,
            deleted_at: DeletedAt::Read,
        }
    }
}

impl<'n, N: CompactNode<'n>> CompactNode<'n> for ProducedNode<N> {
    type Unreadable = N::Unreadable;

    fn shape(self) -> NodeShape<'n, ProducedNode<N>> {
        let deleted_at = self.deleted_at;
        let produced = move |node| ProducedNode { node, deleted_at };
        match self.node.shape() {
            NodeShape::Null => NodeShape::Null,
            NodeShape::Boolean(flag) => NodeShape::Boolean(flag),
            NodeShape::Number(text) => NodeShape::Number(text),
            NodeShape::String(text) => NodeShape::String(text),
            NodeShape::Array(items) => NodeShape::Array(items.into_iter().map(produced).collect()),
            NodeShape::Object(members) => NodeShape::Object(NodeMembers::new(
                members
                    .into_iter()
                    .filter(|(name, _)| !is_broker_generated(name, deleted_at))
                    .map(|(name, node)| (name, produced(node)))
                    .collect(),
            )),
            NodeShape::Geometry(form) => NodeShape::Geometry(form),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, N::Unreadable> {
        self.node.literal()
    }

    fn decoded_by<T, S: for<'de> DeserializeSeed<'de, Value = T>>(self, seed: S) -> Result<Result<T, serde_json::Error>, N::Unreadable> {
        self.node.decoded_by(seed)
    }

    fn is_null(self) -> bool {
        self.node.is_null()
    }

    fn value_budget(self) -> Option<&'n ValueBudget> {
        self.node.value_budget()
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, N::Unreadable> {
        self.node.read_geometry()
    }
}
