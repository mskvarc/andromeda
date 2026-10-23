//! A compact node whose reserved members are read under their core names.
//!
//! A document written under a context that defines another term for a
//! reserved NGSI-LD member writes the member under that term. The node applies
//! [`CoreTermAliases`] while it is walked: each member name is read as the core
//! term it stands for, and so is each string of a `type` member. A value is
//! never renamed inside: what a node holds as JSON is taken from the node as it
//! was written.

use crate::{
    compact_node::{CompactNode, NodeMembers, NodeShape},
    core_term_aliases::{CoreTermAliases, Standing},
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use serde::de::DeserializeOwned;

/// A compact node whose members are read under the core terms their names
/// alias.
#[derive(Debug, Clone, Copy)]
pub struct AliasedNode<'a, N> {
    /// The node as written.
    node: N,
    /// The aliases its members are read through.
    aliases: &'a CoreTermAliases,
    /// What the node stands for.
    standing: Standing,
}

impl<'a, N> AliasedNode<'a, N> {
    /// Read `node` through `aliases`.
    #[must_use]
    pub const fn new(node: N, aliases: &'a CoreTermAliases) -> AliasedNode<'a, N> {
        AliasedNode {
            node,
            aliases,
            standing: Standing::Node,
        }
    }

    /// Read `node`, written in the place `standing` names, through the same
    /// aliases.
    fn standing(self, node: N, standing: Standing) -> AliasedNode<'a, N> {
        AliasedNode {
            node,
            aliases: self.aliases,
            standing,
        }
    }
}

impl<'a: 'n, 'n, N: CompactNode<'n>> CompactNode<'n> for AliasedNode<'a, N> {
    type Unreadable = N::Unreadable;

    fn shape(self) -> NodeShape<'n, AliasedNode<'a, N>> {
        match self.node.shape() {
            NodeShape::Null => NodeShape::Null,
            NodeShape::Boolean(flag) => NodeShape::Boolean(flag),
            NodeShape::Number(text) => NodeShape::Number(text),
            NodeShape::String(text) => NodeShape::String(self.aliases.string(text, self.standing)),
            NodeShape::Array(items) => NodeShape::Array(items.into_iter().map(|item| self.standing(item, self.standing)).collect()),
            NodeShape::Object(members) => NodeShape::Object(NodeMembers::new(
                members
                    .into_iter()
                    .map(|(name, node)| {
                        let name = self.aliases.core_name(name);
                        let standing = Standing::of_member(&name);
                        (name, self.standing(node, standing))
                    })
                    .collect(),
            )),
            NodeShape::Geometry(form) => NodeShape::Geometry(form),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, N::Unreadable> {
        self.node.literal()
    }

    fn decoded<T: DeserializeOwned>(self) -> Result<Result<T, serde_json::Error>, N::Unreadable> {
        self.node.decoded()
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, N::Unreadable> {
        self.node.read_geometry()
    }
}
