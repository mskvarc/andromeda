//! What a write carries (clauses 5.5.8, 5.5.12, 5.6.x).
//!
//! A written fragment is not state. It may name no entity (an attribute
//! fragment carries its entity in the request path), omit members, delete
//! members with the NGSI-LD Null, and repeat a `datasetId` (a Temporal
//! Evolution fragment). [`EntityDocument`] is the typed expanded form of such a
//! write; [`EntityState`](crate::entity_state::EntityState) is built from it
//! only through the validating conversion.

pub mod attribute_member;
pub mod attribute_nesting;
pub mod instance_document;
pub mod slot;
pub mod value_carrier;
pub mod written_value;

use crate::{
    document::{attribute_member::AttributeMember, slot::Slot},
    scopes::Scopes,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier, time::date_time::NgsiLdDateTime};
use indexmap::IndexMap;

/// The typed expanded document of one write.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityDocument {
    /// `@id`, when the document names its entity.
    id: Option<Identifier>,
    /// `@type`, in written order.
    types: Vec<EntityTypeName>,
    /// `scope` (clause 4.18).
    scope: Option<Slot<Scopes>>,
    /// The entity-level `expiresAt` (clause 4.22).
    expires_at: Option<Slot<NgsiLdDateTime>>,
    /// The written attribute members, in written order.
    attributes: IndexMap<AttributeName, AttributeMember>,
}

impl EntityDocument {
    /// Assemble a document from its parts.
    #[must_use]
    pub const fn new(
        id: Option<Identifier>,
        types: Vec<EntityTypeName>,
        scope: Option<Slot<Scopes>>,
        expires_at: Option<Slot<NgsiLdDateTime>>,
        attributes: IndexMap<AttributeName, AttributeMember>,
    ) -> EntityDocument {
        EntityDocument {
            id,
            types,
            scope,
            expires_at,
            attributes,
        }
    }

    /// `@id`.
    #[must_use]
    pub const fn id(&self) -> Option<&Identifier> {
        self.id.as_ref()
    }

    /// `@type`.
    #[must_use]
    pub fn types(&self) -> &[EntityTypeName] {
        &self.types
    }

    /// `scope`.
    #[must_use]
    pub const fn scope(&self) -> Option<&Slot<Scopes>> {
        self.scope.as_ref()
    }

    /// The entity-level `expiresAt`.
    #[must_use]
    pub const fn expires_at(&self) -> Option<&Slot<NgsiLdDateTime>> {
        self.expires_at.as_ref()
    }

    /// The written attribute members.
    #[must_use]
    pub const fn attributes(&self) -> &IndexMap<AttributeName, AttributeMember> {
        &self.attributes
    }

    /// The written attribute members, for a mutation.
    pub const fn attributes_mut(&mut self) -> &mut IndexMap<AttributeName, AttributeMember> {
        &mut self.attributes
    }

    /// Give the document up as its members.
    #[must_use]
    pub fn into_parts(self) -> EntityDocumentParts {
        EntityDocumentParts {
            id: self.id,
            types: self.types,
            scope: self.scope,
            expires_at: self.expires_at,
            attributes: self.attributes,
        }
    }
}

/// The members of a written entity, given up by [`EntityDocument::into_parts`]
/// so a conversion moves them rather than copying them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityDocumentParts {
    /// `@id`.
    pub id: Option<Identifier>,
    /// `@type`, in written order.
    pub types: Vec<EntityTypeName>,
    /// `scope`.
    pub scope: Option<Slot<Scopes>>,
    /// The entity-level `expiresAt`.
    pub expires_at: Option<Slot<NgsiLdDateTime>>,
    /// The written attribute members.
    pub attributes: IndexMap<AttributeName, AttributeMember>,
}
