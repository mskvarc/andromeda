//! A compact node a Context Producer wrote, read without the members it may
//! not supply.
//!
//! Clause 5.2.2 makes `createdAt`, `modifiedAt` and `deletedAt` system
//! generated: "They shall not be provided by Context Producers. In the event
//! that they are provided ... implementations shall ignore them". A request is
//! therefore read as if it never named them, on the entity and on every
//! attribute instance alike, whatever they hold; a value is never filtered
//! inside, since what a node holds as JSON is taken from the node as written.

use crate::compact_node::{CompactNode, NodeMembers, NodeShape};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde::de::DeserializeOwned;

/// The members clause 5.2.2 has the broker generate.
const SYSTEM_GENERATED: [&str; 3] = [compact_properties::CREATED_AT, compact_properties::MODIFIED_AT, compact_properties::DELETED_AT];

/// A compact node a Context Producer wrote.
#[derive(Debug, Clone, Copy)]
pub struct ProducedNode<N>(N);

impl<N> ProducedNode<N> {
    /// Read `node` as a Context Producer's.
    #[must_use]
    pub const fn new(node: N) -> ProducedNode<N> {
        ProducedNode(node)
    }
}

impl<'n, N: CompactNode<'n>> CompactNode<'n> for ProducedNode<N> {
    type Unreadable = N::Unreadable;

    fn shape(self) -> NodeShape<'n, ProducedNode<N>> {
        match self.0.shape() {
            NodeShape::Null => NodeShape::Null,
            NodeShape::Boolean(flag) => NodeShape::Boolean(flag),
            NodeShape::Number(text) => NodeShape::Number(text),
            NodeShape::String(text) => NodeShape::String(text),
            NodeShape::Array(items) => NodeShape::Array(items.into_iter().map(ProducedNode).collect()),
            NodeShape::Object(members) => NodeShape::Object(NodeMembers::new(
                members
                    .into_iter()
                    .filter(|(name, _)| !SYSTEM_GENERATED.contains(&name.as_ref()))
                    .map(|(name, node)| (name, ProducedNode(node)))
                    .collect(),
            )),
            NodeShape::Geometry(form) => NodeShape::Geometry(form),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, N::Unreadable> {
        self.0.literal()
    }

    fn decoded<T: DeserializeOwned>(self) -> Result<Result<T, serde_json::Error>, N::Unreadable> {
        self.0.decoded()
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, N::Unreadable> {
        self.0.read_geometry()
    }
}
