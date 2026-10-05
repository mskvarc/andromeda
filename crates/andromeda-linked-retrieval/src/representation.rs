//! How a node of one representation is read by the engine: spelling only.
//!
//! The engine decides everything about Linked Entities over abstract nodes. A
//! representation answers the four questions whose answer depends on how a
//! node is spelled -- its identifier, the edges it names, its attribute member
//! keys and which of the clause 4.21 keyword members it carries -- and decides
//! nothing.

use crate::{edge::LinkedEdge, keyword::Keyword};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use enumset::EnumSet;
use std::fmt::Debug;

/// The spelling of one kind of node the engine joins.
pub trait LinkedRepresentation {
    /// The node: one entity as this representation holds it.
    type Node: Debug;

    /// The node's identifier, or `None` when it carries none an entity could
    /// be keyed by.
    fn identifier(node: &Self::Node) -> Option<Identifier>;

    /// Append, in member order, instance order and object order, every edge
    /// the node's Relationships name.
    fn edges(node: &Self::Node, out: &mut Vec<LinkedEdge>);

    /// The node's attribute member keys, in member order.
    fn attribute_keys(node: &Self::Node) -> impl Iterator<Item = &str>;

    /// Which of `id`, `type` and `scope` the node carries.
    fn keyword_members(node: &Self::Node) -> EnumSet<Keyword>;
}
