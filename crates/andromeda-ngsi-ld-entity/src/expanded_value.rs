//! The typed expanded value of a Property, a `ListProperty` item, or a member
//! of a structured value.
//!
//! JSON-LD expansion turns a scalar into a value object, a JSON object into a
//! node whose members are expanded names (see
//! [`crate::expanded_value_name`]), an array under a `@list` container
//! into a list object, and a value coerced to `@json` into a JSON literal. The
//! four are the variants here; nothing else can stand in a value position.

use crate::{
    expanded_value_name::ExpandedValueName,
    literal::Literal,
    node_id::NodeId,
    value_mode::{Stored, ValueMode},
};
use andromeda_json_literal::JsonLiteral;
use indexmap::IndexMap;

/// One expanded value.
#[derive(Clone, Debug, PartialEq)]
pub enum ExpandedValue<M: ValueMode = Stored> {
    /// A value object: one scalar with its datatype or language.
    Literal(Literal),
    /// A JSON literal (`"@type": "@json"`).
    Json(JsonLiteral),
    /// A node object or a node reference.
    Node(ValueNode<M>),
    /// A list object (`@list`), in order.
    List(Vec<ExpandedValue<M>>),
}

/// A node of a structured value: what expansion makes of a JSON object.
///
/// A node holding only an identifier is a node reference.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueNode<M: ValueMode = Stored> {
    /// The node's `@id`.
    id: Option<NodeId>,
    /// The node's `@type` entries, in order.
    types: Vec<ExpandedValueName>,
    /// The node's members by expanded name, in written order.
    members: IndexMap<ExpandedValueName, M::Member>,
}

impl<M: ValueMode> ValueNode<M> {
    /// Assemble a node from its parts.
    #[must_use]
    pub const fn new(id: Option<NodeId>, types: Vec<ExpandedValueName>, members: IndexMap<ExpandedValueName, M::Member>) -> ValueNode<M> {
        ValueNode { id, types, members }
    }

    /// The node's `@id`, when it has one.
    #[must_use]
    pub const fn id(&self) -> Option<&NodeId> {
        self.id.as_ref()
    }

    /// The node's `@type` entries.
    #[must_use]
    pub fn types(&self) -> &[ExpandedValueName] {
        &self.types
    }

    /// The node's members by expanded name.
    #[must_use]
    pub const fn members(&self) -> &IndexMap<ExpandedValueName, M::Member> {
        &self.members
    }

    /// The node's members, for a mutation.
    pub const fn members_mut(&mut self) -> &mut IndexMap<ExpandedValueName, M::Member> {
        &mut self.members
    }

    /// Whether the node carries only an identifier.
    #[must_use]
    pub fn is_reference(&self) -> bool {
        self.id.is_some() && self.types.is_empty() && self.members.is_empty()
    }

    /// Give the node up as its parts.
    #[must_use]
    pub fn into_parts(self) -> (Option<NodeId>, Vec<ExpandedValueName>, IndexMap<ExpandedValueName, M::Member>) {
        (self.id, self.types, self.members)
    }
}

impl ValueNode<Stored> {
    /// The values of the member `name`, read by expanded name (the clause 4.9
    /// attribute path into a structured value).
    #[must_use]
    pub fn member(&self, name: &str) -> Option<&[ExpandedValue]> {
        self.members.get(name).map(Vec::as_slice)
    }
}
