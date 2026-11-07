//! The Temporal Evolution of an Entity in the compact normalized
//! representation (ETSI GS CIM 009 clauses 4.5.6 and 4.5.7).
//!
//! The entity keeps its own members, and every Attribute is replaced by its
//! temporal representation: the sequence of instances it took over time
//! ([`TemporalAttribute`]). The Scope is one of those Attributes, a Property
//! whose value is the Scope path or paths in force from the instant it names
//! (clause 4.18 and annex C, clause C.5.16).
//!
//! The reading is the compact model's one reading of a node
//! ([`CompactNode`]), so a temporal entity a peer answered with, one the walk
//! of compaction wrote and one a client wrote are read by the same rules.

use crate::{
    attribute::{
        instance::AttributeValue,
        member_decoding::refusal,
        member_value_reading::{ReadError, optional_instant_member},
    },
    compact_member::decoded,
    entity::Entity,
    entity_system_times::EntitySystemTimes,
    entity_type::EntityTypeValue,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
    temporal::temporal_attribute::TemporalAttribute,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use std::str::FromStr;

/// A Temporal Evolution of an Entity, compact and normalized.
#[derive(Clone, Debug)]
pub struct TemporalEntity {
    /// The entity's identifier (clause 4.5.1).
    pub id: Identifier,
    /// The entity's types.
    pub type_: EntityTypeValue,
    /// The reified Scope history (clause 4.18).
    pub scope: Option<TemporalAttribute>,
    /// The entity's own clause 4.8 system times.
    pub system_times: EntitySystemTimes,
    /// The temporal representation of every other Attribute, in order.
    pub attrs: IndexMap<AttributeName, TemporalAttribute>,
}

impl TemporalEntity {
    /// The Temporal Evolution a compact node is.
    ///
    /// An Attribute whose array holds no instance is no Attribute here (see
    /// [`TemporalAttribute::from_node`]).
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] for a node that is no entity: no
    /// object, a member that does not decode, an Attribute named by no
    /// attribute name or one of whose instances is refused, or a missing `id`
    /// or `type`; and [`NodeReadError::Unreadable`] when a node of it has no
    /// JSON form.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<TemporalEntity, ReadError<'n, N>> {
        let NodeShape::Object(members) = node.shape() else {
            return Err(NodeRefusal::NotAnObject.into());
        };
        let mut id = None;
        let mut type_ = None;
        let mut scope = None;
        let mut system_times = EntitySystemTimes::default();
        let mut attrs: IndexMap<AttributeName, TemporalAttribute> = IndexMap::new();
        for (name, member) in members {
            let at = |error: ReadError<'n, N>| error.within_member(&name);
            match name.as_ref() {
                keyword::CONTEXT | compact_properties::EXPIRES_AT => {}
                keyword::ID_ALIAS => id = Some(required(member, RefusedMember::Id).map_err(at)?),
                keyword::TYPE_ALIAS => type_ = Some(required(member, RefusedMember::Type).map_err(at)?),
                compact_properties::SCOPE => scope = TemporalAttribute::from_node(compact_properties::SCOPE, member).map_err(at)?,
                compact_properties::CREATED_AT => system_times.created_at = optional_instant_member(Some(member), RefusedMember::CreatedAt).map_err(at)?,
                compact_properties::MODIFIED_AT => system_times.modified_at = optional_instant_member(Some(member), RefusedMember::ModifiedAt).map_err(at)?,
                compact_properties::DELETED_AT => system_times.deleted_at = optional_instant_member(Some(member), RefusedMember::DeletedAt).map_err(at)?,
                attribute => {
                    let attribute_name = AttributeName::from_str(attribute).map_err(|source| at(NodeRefusal::AttributeName(source).into()))?;
                    if let Some(read) = TemporalAttribute::from_node(attribute, member).map_err(at)? {
                        attrs.shift_remove(&attribute_name);
                        attrs.insert(attribute_name, read);
                    }
                }
            }
        }
        Ok(TemporalEntity {
            id: id.ok_or(NodeRefusal::Missing { member: RefusedMember::Id })?,
            type_: type_.ok_or(NodeRefusal::Missing { member: RefusedMember::Type })?,
            scope,
            system_times,
            attrs,
        })
    }

    /// This evolution as the compact entity the typed expansion walks, its
    /// system times given apart: every attribute is the multi-attribute of its
    /// instances, and the Scope history is the attribute named `scope`, since
    /// the entity's own `scope` member holds paths and not instances.
    ///
    /// # Errors
    ///
    /// Returns the [`NgsiLdReferenceError`] of a `scope` that is no attribute
    /// name, which it always is.
    pub fn into_entity(self) -> Result<(Entity<AttributeValue>, EntitySystemTimes), NgsiLdReferenceError> {
        let TemporalEntity {
            id,
            type_,
            scope,
            system_times,
            attrs,
        } = self;
        let mut members = IndexMap::with_capacity(attrs.len() + usize::from(scope.is_some()));
        if let Some(history) = scope {
            members.insert(AttributeName::from_str(compact_properties::SCOPE)?, history.into_member());
        }
        members.extend(attrs.into_iter().map(|(name, attribute)| (name, attribute.into_member())));
        let entity = Entity {
            id,
            type_,
            scope: None,
            expires_at: None,
            location: None,
            observation_space: None,
            operation_space: None,
            created_at: None,
            modified_at: None,
            deleted_at: None,
            attrs: members,
        };
        Ok((entity, system_times))
    }

    /// The entity's attributes in order, the Scope history first when it has
    /// one, each with the name it is written under.
    pub fn attribute_members(&self) -> impl Iterator<Item = (&str, &TemporalAttribute)> {
        self.scope
            .iter()
            .map(|scope| (compact_properties::SCOPE, scope))
            .chain(self.attrs.iter().map(|(name, attribute)| (name.as_str(), attribute)))
    }
}

/// Read a member the entity requires, `null` being no value.
fn required<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<T, ReadError<'n, N>> {
    decoded(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

// Hand-written: the entity is read by the compact model's reading of a node,
// which no derive expresses.
impl<'de> Deserialize<'de> for TemporalEntity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<TemporalEntity, D::Error> {
        let literal = JsonLiteral::deserialize(deserializer)?;
        TemporalEntity::from_node(literal.piece()).map_err(refusal)
    }
}

#[cfg(test)]
mod tests;
