//! What a traversal reached: the one graph every narrowing and nesting
//! decision reads.

use crate::{edge::LinkedEdge, representation::LinkedRepresentation};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use indexmap::IndexMap;
use std::num::NonZeroU32;

/// The entities a traversal starts from: their identifiers and the edges
/// their Relationships name.
///
/// The edges are handed in rather than read off the roots, because a consumer
/// may hold a root in a narrowed spelling that no longer shows a Relationship
/// it walks.
#[derive(Debug, Default)]
pub struct RootEdges {
    /// The roots' identifiers, in payload order.
    pub ids: Vec<Identifier>,
    /// Every edge the roots name, in payload order.
    pub edges: Vec<LinkedEdge>,
}

impl RootEdges {
    /// The identifiers and edges of `roots` as their representation spells
    /// them.
    pub fn of<'n, R>(roots: impl IntoIterator<Item = &'n R::Node>) -> RootEdges
    where
        R: LinkedRepresentation,
        R::Node: 'n,
    {
        let mut ids = Vec::new();
        let mut edges = Vec::new();
        for root in roots {
            ids.extend(R::identifier(root));
            R::edges(root, &mut edges);
        }
        RootEdges { ids, edges }
    }
}

/// The Linked Entities a traversal resolved, the edges between them and the
/// roots, and what it could not resolve.
///
/// Immutable once traversed apart from the consumer's own spelling of the
/// nodes, so one graph can serve every notification of one change.
#[derive(Debug)]
pub struct LinkedGraph<N> {
    /// The roots' identifiers, in payload order.
    pub roots: Vec<Identifier>,
    /// The resolved Linked Entities, in the order the traversal resolved them:
    /// level by level, local before remote within a level.
    pub nodes: IndexMap<Identifier, N>,
    /// Every edge the roots and the resolved nodes name.
    pub edges: Vec<LinkedEdge>,
    /// Every Linked Entity the traversal left unresolved.
    pub failures: Vec<LinkedResolutionFailure>,
    /// How many hops from the roots the traversal walked.
    pub depth: NonZeroU32,
}

impl<N> LinkedGraph<N> {
    /// Every attribute key a narrowing of this graph matches a projection
    /// against: those naming an edge, and those of every node.
    pub fn attribute_keys<R>(&self) -> impl Iterator<Item = &str>
    where
        R: LinkedRepresentation<Node = N>,
    {
        self.edges
            .iter()
            .map(|edge| edge.attribute.as_str())
            .chain(self.nodes.values().flat_map(R::attribute_keys))
    }
}
