//! A notified entity on its way into the subscription's `notification.format`
//! (clause 5.8.6).
//!
//! The entity stays the typed compact entity it was rendered as. What the
//! notification reports of the change beside it -- the deletion of every
//! Attribute an entity deletion took, the instances a change deleted, the
//! previous values of `showChanges` -- is kept as the typed renderings it is
//! read from, and the walk a query response is written by writes all of it,
//! the Linked Entities of a `join=inline` subscription included.

use andromeda_linked_retrieval::{
    plan::{FlatEntity, InlineCursor},
    typed::TypedEntities,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_render::{
    notification_entity::{NotificationEntity, StampedInstants},
    notification_language::NotificationLanguage,
    notified_changes::{DeletedInstances, HeldAttributes, NotifiedChanges},
    structural_projection::StructuralProjection,
    version::fallback::VersionFallback,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// How every entity of one notification is written: the subscription's
/// `notification.format` and the clause 4.3.6.8 fallback of its
/// `ngsildConformance`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WrittenShape<'a> {
    /// The `notification.format`.
    pub(crate) format: ResponseFormat,
    /// The fallback every entity is amended by, if the subscription names an
    /// older version.
    pub(crate) fallback: Option<VersionFallback>,
    /// The subscription's `lang`, which every entity is reduced under.
    pub(crate) language: Option<&'a NotificationLanguage>,
}

/// The instances a change deleted, as the state before it held them.
#[derive(Debug)]
pub(crate) struct DeletedRendering {
    /// The rendering of the deleted instances.
    pub(crate) instances: Arc<Entity>,
    /// The `deletedAt` each is stamped with, when the subscription asks for
    /// system attributes.
    pub(crate) deleted_at: Option<DateTime<Utc>>,
}

/// The notified entity, and what the notification reports beside its state.
#[derive(Debug)]
pub(crate) struct NotifiedEntity {
    /// The compact entity.
    pub(crate) entity: Arc<Entity>,
    /// The entity times written on it.
    pub(crate) stamped: StampedInstants,
    /// What became of the Attributes it holds.
    pub(crate) held: HeldAttributes,
    /// The instances the change deleted, if the notification reports any.
    pub(crate) deleted: Option<DeletedRendering>,
    /// Under `showChanges`, what the change touched as it was before.
    pub(crate) previous: Option<Arc<Entity>>,
}

impl NotifiedEntity {
    /// What the notification reports beside the entity's state, `None` when
    /// it reports nothing beyond it.
    fn changes(&self) -> Option<NotifiedChanges<'_>> {
        let reports_changes = self.held == HeldAttributes::Deleted || self.deleted.is_some() || self.previous.is_some();
        reports_changes.then(|| NotifiedChanges {
            held: self.held,
            deleted: self.deleted.as_ref().map(|deleted| DeletedInstances {
                instances: &deleted.instances,
                deleted_at: deleted.deleted_at,
            }),
            previous: self.previous.as_deref(),
        })
    }

    /// The entity, to be written as `shape` says, `type` and `scope` narrowed
    /// by `projection`, the Linked Entities the plan's cursor answers nested
    /// into it.
    pub(crate) fn notified<'a>(
        &'a self,
        shape: WrittenShape<'a>,
        join: Option<InlineCursor<'a, TypedEntities>>,
        projection: StructuralProjection<'a>,
    ) -> NotificationEntity<'a> {
        let notified = NotificationEntity::new(&self.entity, &self.stamped, shape.format)
            .with_projection(projection)
            .with_language(shape.language)
            .with_fallback(shape.fallback);
        let notified = match self.changes() {
            Some(changes) => notified.with_changes(changes),
            None => notified,
        };
        match join {
            Some(join) => notified.with_inline_join(join),
            None => notified,
        }
    }
}

/// One Linked Entity a flat join appends (clause 4.5.23.3), to be written as
/// `shape` says, by the rule its first encounter decided.
pub(crate) const fn linked_entity<'a>(linked: &FlatEntity<'a, TypedEntities>, shape: WrittenShape<'a>) -> NotificationEntity<'a> {
    NotificationEntity::linked(linked, shape.format)
        .with_fallback(shape.fallback)
        .with_language(shape.language)
}
