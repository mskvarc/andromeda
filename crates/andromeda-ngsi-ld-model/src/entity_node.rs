//! Reading a whole [`Entity`] from a compact node (ETSI GS CIM 009 clause
//! 5.2.4).
//!
//! Each member is placed by the name it is written under, as the decoding of a
//! document places it ([`EntityMember::named`]), decoded by the type the
//! entity holds it in, and each attribute member is read by
//! [`AttributeValue::from_node`].

use crate::{
    attribute::instance::AttributeValue,
    compact_member::{decoded, decoded_optional},
    compact_node::{CompactNode, NodeShape},
    entity::Entity,
    entity_member::{EntityForm, EntityMember, SharedMembers},
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
};
use serde::de::DeserializeOwned;

/// The failure of reading a member of a node of type `N`.
type ReadError<N> = NodeReadError<<N as CompactNode>::Unreadable>;

impl Entity {
    /// The entity a compact node is.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] for a node that is no entity: no
    /// object, an entity member that does not decode, an attribute named by no
    /// attribute name, or a missing `id` or `type`; and
    /// [`NodeReadError::Unreadable`] when a node of it has no JSON form.
    pub fn from_node<N: CompactNode>(node: &N) -> Result<Entity, ReadError<N>> {
        let NodeShape::Object(members) = node.shape() else {
            return Err(NodeRefusal::NotAnObject.into());
        };
        let mut read = SharedMembers::default();
        for (name, member) in members {
            match EntityMember::named(name, EntityForm::Entity).map_err(NodeRefusal::AttributeName)? {
                EntityMember::Id => read.id = Some(required(member, RefusedMember::Id)?),
                EntityMember::Type => read.type_ = Some(required(member, RefusedMember::Type)?),
                EntityMember::Scope => read.scope = optional(member, RefusedMember::Scope)?,
                EntityMember::ExpiresAt => read.expires_at = optional(member, RefusedMember::ExpiresAt)?,
                EntityMember::CreatedAt => read.created_at = optional(member, RefusedMember::CreatedAt)?,
                EntityMember::ModifiedAt => read.modified_at = optional(member, RefusedMember::ModifiedAt)?,
                EntityMember::DeletedAt => read.deleted_at = optional(member, RefusedMember::DeletedAt)?,
                EntityMember::Location => read.location = optional_attribute(member)?,
                EntityMember::ObservationSpace => read.observation_space = optional_attribute(member)?,
                EntityMember::OperationSpace => read.operation_space = optional_attribute(member)?,
                EntityMember::Attribute(name) => {
                    let value = AttributeValue::from_node(member).map_err(NodeReadError::Unreadable)?;
                    read.attrs.insert(name, value);
                }
            }
        }
        read.into_entity().map_err(|missing| NodeRefusal::Missing { member: missing.member() }.into())
    }
}

/// Decode a member the entity holds as a `T`.
fn required<T: DeserializeOwned, N: CompactNode>(node: &N, name: RefusedMember) -> Result<T, ReadError<N>> {
    decoded(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

/// Decode a member the entity holds as an optional `T`: `null` is no value.
fn optional<T: DeserializeOwned, N: CompactNode>(node: &N, name: RefusedMember) -> Result<Option<T>, ReadError<N>> {
    decoded_optional(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

/// Read one of the clause 5.2.4 geo-property members: `null` is no member.
fn optional_attribute<N: CompactNode>(node: &N) -> Result<Option<AttributeValue>, ReadError<N>> {
    match node.shape() {
        NodeShape::Null => Ok(None),
        NodeShape::Boolean | NodeShape::Number | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            AttributeValue::from_node(node).map(Some).map_err(NodeReadError::Unreadable)
        }
    }
}
