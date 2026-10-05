//! What an entity holds under an attribute name: the members of an entity the
//! broker holds, or the members of a merge fragment (ETSI GS CIM 009 clauses
//! 4.5.5 and 5.5.12).

use crate::node_refusal::NodeReadError;
use andromeda_ngsi_ld_representation::compact_node::CompactNode;

/// An attribute member an entity holds, read from the compact node written
/// under its name.
pub trait MemberReading: Sized {
    /// The member a compact node written under the attribute name `name` is;
    /// the name decides only whether the clause 4.5.2.2 seals are members of a
    /// Property's own.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] for a node no clause admits where an
    /// attribute stands, located from the node, and
    /// [`NodeReadError::Unreadable`] when a node of it has no JSON form.
    fn from_node<'n, N: CompactNode<'n>>(name: &str, node: N) -> Result<Self, NodeReadError<N::Unreadable>>;

    /// The member a bare Attribute Fragment of clauses 5.6.4 and 5.6.19,
    /// written for the attribute named `name`, is. The fragment is the
    /// attribute itself, so an object is read as the instance it is even when
    /// it names neither a `type` nor a value member, which in place of a
    /// member clause 4.5.2.3 would read as a Property value.
    ///
    /// # Errors
    ///
    /// As [`MemberReading::from_node`].
    fn from_fragment_node<'n, N: CompactNode<'n>>(name: &str, fragment: N) -> Result<Self, NodeReadError<N::Unreadable>>;
}
