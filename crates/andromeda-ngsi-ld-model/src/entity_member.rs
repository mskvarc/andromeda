//! The members of an entity and of the partial form a patch accepts, by the
//! name each is written under (ETSI GS CIM 009 clause 5.2.4).
//!
//! The members clause 5.2.4 names are the entity's own; every other member is
//! an attribute, named by an attribute name (clause 4.6.2). A decoded document
//! and a compacted entity are read by these same names into the same
//! [`SharedMembers`], so the two readings cannot place a member differently.

use crate::{
    entity::{Entity, EntityPatch},
    entity_type::EntityTypeValue,
    node_refusal::RefusedMember,
    scope_member::ScopeMember,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::str::FromStr;

/// A member of an entity, by the name it is written under.
#[derive(Debug)]
pub(crate) enum EntityMember {
    /// `id`.
    Id,
    /// `type`.
    Type,
    /// `scope`.
    Scope,
    /// `expiresAt`.
    ExpiresAt,
    /// `location`.
    Location,
    /// `observationSpace`.
    ObservationSpace,
    /// `operationSpace`.
    OperationSpace,
    /// `createdAt`.
    CreatedAt,
    /// `modifiedAt`.
    ModifiedAt,
    /// `deletedAt`.
    DeletedAt,
    /// Any other member: an attribute, by name.
    Attribute(AttributeName),
}

/// Which form of the entity is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntityForm {
    /// The whole entity: `id` and `type` are required, and the system
    /// timestamps are members of their own.
    Entity,
    /// The partial form a patch operation accepts: `id` and `type` may be
    /// `null`, and the system timestamps are attributes like any other member.
    Patch,
}

impl EntityMember {
    /// The member `name` is in `form`.
    ///
    /// # Errors
    ///
    /// Returns the [`NgsiLdReferenceError`] of an attribute member whose name
    /// is no attribute name.
    pub(crate) fn named(name: &str, form: EntityForm) -> Result<EntityMember, NgsiLdReferenceError> {
        Ok(match (name, form) {
            (keyword::ID_ALIAS, EntityForm::Entity | EntityForm::Patch) => EntityMember::Id,
            (keyword::TYPE_ALIAS, EntityForm::Entity | EntityForm::Patch) => EntityMember::Type,
            (compact_properties::SCOPE, EntityForm::Entity | EntityForm::Patch) => EntityMember::Scope,
            (compact_properties::EXPIRES_AT, EntityForm::Entity | EntityForm::Patch) => EntityMember::ExpiresAt,
            (compact_properties::LOCATION, EntityForm::Entity | EntityForm::Patch) => EntityMember::Location,
            (compact_properties::OBSERVATION_SPACE, EntityForm::Entity | EntityForm::Patch) => EntityMember::ObservationSpace,
            (compact_properties::OPERATION_SPACE, EntityForm::Entity | EntityForm::Patch) => EntityMember::OperationSpace,
            (compact_properties::CREATED_AT, EntityForm::Entity) => EntityMember::CreatedAt,
            (compact_properties::MODIFIED_AT, EntityForm::Entity) => EntityMember::ModifiedAt,
            (compact_properties::DELETED_AT, EntityForm::Entity) => EntityMember::DeletedAt,
            (attribute, EntityForm::Entity | EntityForm::Patch) => EntityMember::Attribute(AttributeName::from_str(attribute)?),
        })
    }
}

/// The members every form of the entity shares, read so far.
#[derive(Debug)]
pub(crate) struct SharedMembers<M> {
    /// `id`, when read.
    pub(crate) id: Option<Identifier>,
    /// `type`, when read.
    pub(crate) type_: Option<EntityTypeValue>,
    /// `scope`.
    pub(crate) scope: Option<ScopeMember>,
    /// `expiresAt`.
    pub(crate) expires_at: Option<DateTime<Utc>>,
    /// `location`.
    pub(crate) location: Option<M>,
    /// `observationSpace`.
    pub(crate) observation_space: Option<M>,
    /// `operationSpace`.
    pub(crate) operation_space: Option<M>,
    /// `createdAt`, where the form names it.
    pub(crate) created_at: Option<DateTime<Utc>>,
    /// `modifiedAt`, where the form names it.
    pub(crate) modified_at: Option<DateTime<Utc>>,
    /// `deletedAt`, where the form names it.
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    /// Every attribute member, in the order first written.
    pub(crate) attrs: IndexMap<AttributeName, M>,
}

/// The member a whole entity requires and was not given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MissingMember {
    /// `id`.
    Id,
    /// `type`.
    Type,
}

impl MissingMember {
    /// The member, as a refusal names it.
    pub(crate) const fn member(self) -> RefusedMember {
        match self {
            MissingMember::Id => RefusedMember::Id,
            MissingMember::Type => RefusedMember::Type,
        }
    }
}

// `#[derive(Default)]` would require `M: Default`, which no member type is.
impl<M> Default for SharedMembers<M> {
    fn default() -> SharedMembers<M> {
        SharedMembers {
            id: None,
            type_: None,
            scope: None,
            expires_at: None,
            location: None,
            observation_space: None,
            operation_space: None,
            created_at: None,
            modified_at: None,
            deleted_at: None,
            attrs: IndexMap::new(),
        }
    }
}

impl<M> SharedMembers<M> {
    /// The whole entity these members are.
    ///
    /// # Errors
    ///
    /// Returns the [`MissingMember`] `id` or `type` when it was not read.
    pub(crate) fn into_entity(self) -> Result<Entity<M>, MissingMember> {
        Ok(Entity {
            id: self.id.ok_or(MissingMember::Id)?,
            type_: self.type_.ok_or(MissingMember::Type)?,
            scope: self.scope,
            expires_at: self.expires_at,
            location: self.location,
            observation_space: self.observation_space,
            operation_space: self.operation_space,
            created_at: self.created_at,
            modified_at: self.modified_at,
            deleted_at: self.deleted_at,
            attrs: self.attrs,
        })
    }

    /// The partial entity these members are.
    pub(crate) fn into_patch(self) -> EntityPatch<M> {
        EntityPatch {
            id: self.id,
            type_: self.type_,
            scope: self.scope,
            expires_at: self.expires_at,
            location: self.location,
            observation_space: self.observation_space,
            operation_space: self.operation_space,
            attrs: self.attrs,
        }
    }
}
