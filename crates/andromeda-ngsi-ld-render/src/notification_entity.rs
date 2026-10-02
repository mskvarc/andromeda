//! One entity of a notification's `data`, written straight from its typed
//! form (clause 5.8.6).
//!
//! A notification carries the notified entity in the subscription's
//! `notification.format`, and the entity's own clause 4.8 times the
//! subscription asked for (Table 5.2.14-1) written on it -- and, for a deleted
//! entity, its `deletedAt`. The entity state holds none of those times, so the
//! notification stamps them, spelled with the sub-second precision the instant
//! carries; a time the compact entity carries itself keeps the rendering's own
//! spelling, except `deletedAt`, which is always the deletion's.
//!
//! The entity is written by the typed walk a query response is written by
//! ([`crate::entity_response`]): what the notification reports of the change
//! beside the entity's state ([`NotifiedChanges`]) and the Linked Entities of
//! a `join=inline` subscription (clause 4.5.23.2) are written on the way.

use crate::{
    entity_body::{
        linked_entities::EntityJoin,
        member_walk::MemberWalk,
        typed_entity::{IdentifierMember, TypedEntity},
    },
    entity_response::InlineJoin,
    error::{RenderError, RenderedShape},
    notified_changes::NotifiedChanges,
    structural_projection::StructuralProjection,
    uri_decoding::UriDecoding,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The entity times a notification writes on the notified entity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StampedInstants {
    /// The entity's `createdAt`, written unless the entity carries one.
    pub created_at: Option<DateTime<Utc>>,
    /// The entity's `modifiedAt`, written unless the entity carries one.
    pub modified_at: Option<DateTime<Utc>>,
    /// The entity's `expiresAt`, written unless the entity carries one.
    pub expires_at: Option<DateTime<Utc>>,
    /// The deletion instant of a deleted entity, written whatever the entity
    /// carries.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// How the targets of the entity's Relationships are spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSpelling {
    /// Percent-decoded at the presentation boundary, as a query response
    /// spells them (see [`crate::uri_decoding`]).
    Decoded,
    /// As the store holds them.
    AsStored,
}

/// One notified entity, ready to be written in a notification format.
#[derive(Debug, Clone, Copy)]
pub struct NotificationEntity<'a> {
    /// The compact entity.
    entity: &'a Entity,
    /// The times the notification writes on it.
    stamped: &'a StampedInstants,
    /// The subscription's `notification.format`.
    format: ResponseFormat,
    /// How its Relationship targets are spelled.
    targets: TargetSpelling,
    /// The inline join its Relationships nest their targets by, if the
    /// subscription asked for one.
    join: Option<InlineJoin<'a>>,
    /// What the notification reports of the change beside the entity's state.
    changes: Option<NotifiedChanges<'a>>,
    /// The clause 4.21 rule the subscription's `pick` and `omit` state for
    /// `type` and `scope`; `id` is written whatever it says (clause 5.8.6).
    projection: Option<StructuralProjection<'a>>,
}

impl<'a> NotificationEntity<'a> {
    /// The entity, with `stamped` written on it, in `format`, its
    /// Relationship targets spelled as `targets` says.
    #[must_use]
    pub const fn new(entity: &'a Entity, stamped: &'a StampedInstants, format: ResponseFormat, targets: TargetSpelling) -> NotificationEntity<'a> {
        NotificationEntity {
            entity,
            stamped,
            format,
            targets,
            join: None,
            changes: None,
            projection: None,
        }
    }

    /// The entity, its Relationships nesting the Linked Entities `join`
    /// resolved (clause 4.5.23.2).
    #[must_use]
    pub const fn with_inline_join(self, join: InlineJoin<'a>) -> NotificationEntity<'a> {
        NotificationEntity { join: Some(join), ..self }
    }

    /// The entity, with what the notification reports of the change beside
    /// its state (clause 5.8.6).
    #[must_use]
    pub const fn with_changes(self, changes: NotifiedChanges<'a>) -> NotificationEntity<'a> {
        NotificationEntity {
            changes: Some(changes),
            ..self
        }
    }

    /// The entity, its `type` and `scope` narrowed by the clause 4.21 rule
    /// `projection` states.
    #[must_use]
    pub const fn with_projection(self, projection: StructuralProjection<'a>) -> NotificationEntity<'a> {
        NotificationEntity {
            projection: Some(projection),
            ..self
        }
    }

    /// The entity as the JSON value the notification carries.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the entity cannot be
    /// written.
    pub fn render(&self) -> Result<Value, RenderError> {
        let decoding = match self.targets {
            TargetSpelling::Decoded => UriDecoding::Decode,
            TargetSpelling::AsStored => UriDecoding::Keep,
        };
        let typed = TypedEntity {
            entity: self.entity,
            format: self.format,
            walk: MemberWalk::new(decoding, None),
            projection: self.projection,
            identifier: IdentifierMember::Mandatory,
            context: None,
            stamped: Some(self.stamped),
            join: self.join.map(|join| EntityJoin::root(join.linked, join.projection)),
            narrowing: None,
            changes: self.changes,
        };
        serde_json::to_value(typed).map_err(|error| RenderError::serialization(RenderedShape::Entity, error))
    }
}
