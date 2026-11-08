//! One entity of a notification's `data`, written straight from its typed
//! form (clause 5.8.6).
//!
//! A notification carries the notified entity in the subscription's
//! `notification.format`, and the entity's own clause 4.8 times the
//! subscription asked for (Table 5.2.14-1) written on it, and for a deleted
//! entity, its `deletedAt`. The entity state holds none of those times, so the
//! notification stamps them, spelled with the sub-second precision the instant
//! carries; a time the compact entity carries itself keeps the rendering's own
//! spelling, except `deletedAt`, which is always the deletion's.
//!
//! The entity is written by the typed walk a query response is written by:
//! what the notification reports of the change beside the entity's state
//! ([`NotifiedChanges`]) and the Linked Entities of a `join=inline`
//! subscription (clause 4.5.23.2) are written on the way. A Linked Entity a
//! `join=flat` subscription appends is written by the same walk, under the
//! rule the join plan decided for it.
//!
//! A Subscription's `ngsildConformance` has every Entity of the notification
//! "amended to conform to the supplied version" (clause 5.8.6): the clause
//! 4.3.6.8 fallback is applied by the same walk, to the notified entity, to
//! the Linked Entities nested into it, and to each flat Linked Entity. It
//! reaches the Entities and never the Notification that carries them: Tables
//! 4.3.6.8-1 to -3 describe Entity data alone.

use crate::{
    entity_body::{
        member_walk::MemberWalk,
        typed_entity::{IdentifierMember, TypedEntity},
    },
    notification_language::NotificationLanguage,
    notified_changes::NotifiedChanges,
    structural_projection::StructuralProjection,
    version::fallback::VersionFallback,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_linked_retrieval::{
    narrowing::member::MemberRule,
    plan::{FlatEntity, InlineCursor},
    typed::TypedEntities,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};

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

/// One notified entity, ready to be written in a notification format.
#[derive(Debug, Clone, Copy)]
pub struct NotificationEntity<'a> {
    /// The compact entity.
    entity: &'a Entity,
    /// The times the notification writes on it, if it is a notified entity.
    stamped: Option<&'a StampedInstants>,
    /// The subscription's `notification.format`.
    format: ResponseFormat,
    /// The inline join its Relationships nest their targets by, if the
    /// subscription asked for one.
    join: Option<InlineCursor<'a, TypedEntities>>,
    /// What the notification reports of the change beside the entity's state.
    changes: Option<NotifiedChanges<'a>>,
    /// The clause 4.21 rule the subscription's `pick` and `omit` state for
    /// `id`, `type` and `scope` (clause 5.8.6).
    projection: Option<StructuralProjection<'a>>,
    /// The rule a flat Linked Entity's members are written by, if it narrows
    /// anything.
    narrowing: Option<MemberRule<'a>>,
    /// The clause 4.3.6.8 fallback of the subscription's
    /// `ngsildConformance`, if it names an older version.
    fallback: Option<VersionFallback>,
    /// The clause 4.15 `lang` filter of the subscription (Table 5.2.12-1)
    /// every top-level `LanguageProperty` is negotiated against, if it names
    /// one.
    language: Option<&'a NotificationLanguage>,
}

impl<'a> NotificationEntity<'a> {
    /// The entity, with `stamped` written on it, in `format`.
    #[must_use]
    pub const fn new(entity: &'a Entity, stamped: &'a StampedInstants, format: ResponseFormat) -> NotificationEntity<'a> {
        NotificationEntity {
            entity,
            stamped: Some(stamped),
            format,
            join: None,
            changes: None,
            projection: None,
            narrowing: None,
            fallback: None,
            language: None,
        }
    }

    /// One Linked Entity a flat join appends (clause 4.5.23.3), in `format`,
    /// written by the rule its first encounter decided.
    ///
    /// A Linked Entity keeps its `id` only where its list does (clause
    /// 5.7.1.5), as in a query response.
    #[must_use]
    pub const fn linked(linked: &FlatEntity<'a, TypedEntities>, format: ResponseFormat) -> NotificationEntity<'a> {
        NotificationEntity {
            entity: linked.node,
            stamped: None,
            format,
            join: None,
            changes: None,
            projection: None,
            narrowing: linked.rule.restricting(),
            fallback: None,
            language: None,
        }
    }

    /// One entity a notification relays from its source, re-assembled for
    /// the local subscription (clause 5.8.6), written as a notification of
    /// that subscription writes a local entity: in `format`, `id`, `type` and
    /// `scope` under the subscription's `projection`; its top-level
    /// `LanguageProperty` attributes are reduced under the subscription's
    /// `lang` when it names one (Table 5.2.12-1, clause 4.15).
    ///
    /// The relayed entity carries the times its source wrote on it.
    #[must_use]
    pub const fn relayed(
        entity: &'a Entity,
        format: ResponseFormat,
        projection: StructuralProjection<'a>,
        language: Option<&'a NotificationLanguage>,
    ) -> NotificationEntity<'a> {
        NotificationEntity {
            entity,
            stamped: None,
            format,
            join: None,
            changes: None,
            projection: Some(projection),
            narrowing: None,
            fallback: None,
            language,
        }
    }

    /// The entity, its Relationships nesting the Linked Entities `join`
    /// resolved (clause 4.5.23.2).
    #[must_use]
    pub const fn with_inline_join(self, join: InlineCursor<'a, TypedEntities>) -> NotificationEntity<'a> {
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

    /// The entity, its `id`, `type` and `scope` narrowed by the clause 4.21 rule
    /// `projection` states.
    #[must_use]
    pub const fn with_projection(self, projection: StructuralProjection<'a>) -> NotificationEntity<'a> {
        NotificationEntity {
            projection: Some(projection),
            ..self
        }
    }

    /// The entity, its top-level `LanguageProperty` attributes reduced under
    /// the subscription's `lang` when it names one (Table 5.2.12-1, clause
    /// 4.15).
    #[must_use]
    pub const fn with_language(self, language: Option<&'a NotificationLanguage>) -> NotificationEntity<'a> {
        NotificationEntity { language, ..self }
    }

    /// The entity, amended to the version its subscription's
    /// `ngsildConformance` names (clause 5.8.6), when `fallback` is one.
    #[must_use]
    pub const fn with_fallback(self, fallback: Option<VersionFallback>) -> NotificationEntity<'a> {
        NotificationEntity { fallback, ..self }
    }

    /// The entity as the typed walk writes it, carrying `context` beside its
    /// own members when the body carries one.
    pub(crate) fn typed(&self, context: Option<&'a JsonLiteral>) -> TypedEntity<'a> {
        TypedEntity {
            entity: self.entity,
            format: self.format,
            walk: MemberWalk::new(self.language.map(NotificationLanguage::filter), self.fallback),
            projection: self.projection,
            identifier: IdentifierMember::Written,
            context,
            stamped: self.stamped,
            join: self.join,
            narrowing: self.narrowing,
            changes: self.changes,
        }
    }
}

/// The entity as one member of a notification's `data` writes it, without a
/// `@context` of its own.
impl Serialize for NotificationEntity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.typed(None).serialize(serializer)
    }
}
