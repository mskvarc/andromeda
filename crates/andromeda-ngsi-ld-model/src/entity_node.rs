//! Reading a whole [`Entity`], or the partial [`EntityPatch`], from a compact
//! node (ETSI GS CIM 009 clause 5.2.4).
//!
//! Each member is placed by the name it is written under
//! ([`EntityMember::named`]), decoded by the type the entity holds it in, and
//! each attribute member is read by [`MemberReading::from_node`], or by
//! [`MemberReading::from_fragment_node`] where every attribute is a bare
//! Attribute Fragment. A refusal is located from the top of the entity.

use crate::{
    attribute::{instance_member::ReadError, member_reading::MemberReading},
    compact_member::{decoded, decoded_optional},
    entity::{Entity, EntityPatch},
    entity_member::{EntityForm, EntityMember, SharedMembers},
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use serde::de::DeserializeOwned;

/// The member a JSON-LD document names its context in.
const CONTEXT_MEMBER: &str = "@context";

impl<M: MemberReading> Entity<M> {
    /// The entity a compact node is.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] for a node that is no entity: no
    /// object, an entity member that does not decode, an attribute named by no
    /// attribute name or refused as an attribute, or a missing `id` or `type`;
    /// and [`NodeReadError::Unreadable`] when a node of it has no JSON form.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<Entity<M>, ReadError<'n, N>> {
        shared_members(node, EntityForm::Entity, AttributeReading::Member)?
            .into_entity()
            .map_err(|missing| NodeRefusal::Missing { member: missing.member() }.into())
    }
}

impl<M: MemberReading> EntityPatch<M> {
    /// The partial entity a compact node is.
    ///
    /// # Errors
    ///
    /// As [`Entity::from_node`], except that no member is required.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<EntityPatch<M>, ReadError<'n, N>> {
        shared_members(node, EntityForm::Patch, AttributeReading::Member).map(SharedMembers::into_patch)
    }

    /// The partial entity a compact node is when every attribute it holds is
    /// the bare Attribute Fragment of clauses 5.6.4 and 5.6.19: an object is
    /// the instance it updates even when it names neither a `type` nor a value
    /// member.
    ///
    /// # Errors
    ///
    /// As [`EntityPatch::from_node`].
    pub fn from_fragment_node<'n, N: CompactNode<'n>>(node: N) -> Result<EntityPatch<M>, ReadError<'n, N>> {
        shared_members(node, EntityForm::Patch, AttributeReading::Fragment).map(SharedMembers::into_patch)
    }
}

/// How an attribute member of an entity node is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttributeReading {
    /// As a member written where an attribute stands.
    Member,
    /// As the bare Attribute Fragment a request path names the attribute of.
    Fragment,
}

impl AttributeReading {
    /// The member `node`, written for the attribute `name`, is.
    fn read<'n, M: MemberReading, N: CompactNode<'n>>(self, name: &str, node: N) -> Result<M, ReadError<'n, N>> {
        match self {
            AttributeReading::Member => M::from_node(name, node),
            AttributeReading::Fragment => M::from_fragment_node(name, node),
        }
    }
}

/// Read every member of an entity node as `form` names them.
fn shared_members<'n, M: MemberReading, N: CompactNode<'n>>(
    node: N,
    form: EntityForm,
    reading: AttributeReading,
) -> Result<SharedMembers<M>, ReadError<'n, N>> {
    let NodeShape::Object(members) = node.shape() else {
        return Err(NodeRefusal::NotAnObject.into());
    };
    let mut read = SharedMembers::default();
    for (name, member) in members {
        // Clause 6.3.5: the `@context` of an `application/ld+json` document is
        // the document's, not a member of the entity it holds.
        if name == CONTEXT_MEMBER {
            continue;
        }
        let at = |error: ReadError<'n, N>| error.within_member(&name);
        match EntityMember::named(&name, form).map_err(|source| at(NodeRefusal::AttributeName(source).into()))? {
            EntityMember::Id => read.id = identity(member, form, RefusedMember::Id).map_err(at)?,
            EntityMember::Type => read.type_ = identity(member, form, RefusedMember::Type).map_err(at)?,
            EntityMember::Scope => read.scope = optional(member, RefusedMember::Scope).map_err(at)?,
            EntityMember::ExpiresAt => read.expires_at = optional(member, RefusedMember::ExpiresAt).map_err(at)?,
            EntityMember::CreatedAt => read.created_at = optional(member, RefusedMember::CreatedAt).map_err(at)?,
            EntityMember::ModifiedAt => read.modified_at = optional(member, RefusedMember::ModifiedAt).map_err(at)?,
            EntityMember::DeletedAt => read.deleted_at = optional(member, RefusedMember::DeletedAt).map_err(at)?,
            EntityMember::Location => read.location = optional_attribute(&name, member, reading).map_err(at)?,
            EntityMember::ObservationSpace => read.observation_space = optional_attribute(&name, member, reading).map_err(at)?,
            EntityMember::OperationSpace => read.operation_space = optional_attribute(&name, member, reading).map_err(at)?,
            EntityMember::Attribute(attribute) => {
                let value = reading.read(&name, member).map_err(at)?;
                read.attrs.shift_remove(&attribute);
                read.attrs.insert(attribute, value);
            }
        }
    }
    Ok(read)
}

/// Decode the `id` or `type` of an entity: required in the whole entity,
/// `null` for none in a patch.
fn identity<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N, form: EntityForm, name: RefusedMember) -> Result<Option<T>, ReadError<'n, N>> {
    match form {
        EntityForm::Entity => decoded(node)
            .map_err(NodeReadError::Unreadable)?
            .map(Some)
            .map_err(|source| NodeRefusal::Member { member: name, source }.into()),
        EntityForm::Patch => optional(node, name),
    }
}

/// Decode a member the entity holds as an optional `T`: `null` is no value.
fn optional<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<Option<T>, ReadError<'n, N>> {
    decoded_optional(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

/// Read one of the clause 5.2.4 geo-property members: `null` is no member.
fn optional_attribute<'n, M: MemberReading, N: CompactNode<'n>>(name: &str, node: N, reading: AttributeReading) -> Result<Option<M>, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Null => Ok(None),
        NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            reading.read(name, node).map(Some)
        }
    }
}
