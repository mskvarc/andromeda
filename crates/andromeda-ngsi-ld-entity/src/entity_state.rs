//! The current state of one entity, as stored (clauses 4.5.1, 5.2.4).
//!
//! The state is context-independent: attribute names and types are expanded
//! IRIs, because a request's `@context` decides how names are written on the
//! wire, not what they are (clause 5.5.7). The entity's own system times
//! (`createdAt`, `modifiedAt` of clause 4.8) are store columns, not members of
//! the stored document, and are not part of it.
//!
//! The NGSI-LD Null cannot appear anywhere in a state: every constructor and
//! the canonical decoder refuse it.

use crate::{attributes::Attributes, entity_types::EntityTypes, scopes::Scopes};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::written_date_time::DateTimeText};

/// One entity.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityState {
    /// `@id`.
    id: Identifier,
    /// `@type` (clause 4.5.1, clause 4.16).
    types: EntityTypes,
    /// `scope` (clause 4.18).
    scope: Option<Scopes>,
    /// The entity-level `expiresAt` (clause 4.22).
    expires_at: Option<DateTimeText>,
    /// The attributes.
    attributes: Attributes,
}

impl EntityState {
    /// An entity with no scope, no expiry and no attribute.
    #[must_use]
    pub fn new(id: Identifier, types: EntityTypes) -> EntityState {
        EntityState {
            id,
            types,
            scope: None,
            expires_at: None,
            attributes: Attributes::new(),
        }
    }

    /// Assemble an entity from its parts.
    #[must_use]
    pub const fn from_parts(
        id: Identifier,
        types: EntityTypes,
        scope: Option<Scopes>,
        expires_at: Option<DateTimeText>,
        attributes: Attributes,
    ) -> EntityState {
        EntityState {
            id,
            types,
            scope,
            expires_at,
            attributes,
        }
    }

    /// The entity identifier.
    #[must_use]
    pub const fn id(&self) -> &Identifier {
        &self.id
    }

    /// The entity types, in order.
    #[must_use]
    pub const fn types(&self) -> &EntityTypes {
        &self.types
    }

    /// The scopes, when the entity has any.
    #[must_use]
    pub const fn scope(&self) -> Option<&Scopes> {
        self.scope.as_ref()
    }

    /// The entity-level expiry, when set.
    #[must_use]
    pub const fn expires_at(&self) -> Option<&DateTimeText> {
        self.expires_at.as_ref()
    }

    /// The attributes.
    #[must_use]
    pub const fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    /// The entity types, for a mutation.
    pub const fn types_mut(&mut self) -> &mut EntityTypes {
        &mut self.types
    }

    /// Replace the scopes.
    pub fn set_scope(&mut self, scope: Option<Scopes>) {
        self.scope = scope;
    }

    /// Replace the entity-level expiry.
    pub fn set_expires_at(&mut self, expires_at: Option<DateTimeText>) {
        self.expires_at = expires_at;
    }

    /// The attributes, for a mutation.
    pub const fn attributes_mut(&mut self) -> &mut Attributes {
        &mut self.attributes
    }

    /// Give the entity up as its parts.
    #[must_use]
    pub fn into_parts(self) -> (Identifier, EntityTypes, Option<Scopes>, Option<DateTimeText>, Attributes) {
        (self.id, self.types, self.scope, self.expires_at, self.attributes)
    }
}
