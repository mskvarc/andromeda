//! A notified entity on its way into the subscription's `notification.format`
//! (clause 5.8.6).
//!
//! The entity stays the typed compact entity it was rendered as. What the
//! notification reports of the change beside it -- the deletion of every
//! Attribute an entity deletion took, the instances a change deleted, the
//! previous values of `showChanges` -- is kept as the typed renderings it is
//! read from, and the walk a query response is written by writes all of it,
//! the Linked Entities of a `join=inline` subscription included.

use crate::error::NotificationError;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_render::{
    entity_response::InlineJoin,
    notification_entity::{NotificationEntity, StampedInstants, TargetSpelling},
    notified_changes::{DeletedInstances, HeldAttributes, NotifiedChanges},
    structural_projection::StructuralProjection,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

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

    /// The entity in `format`, its Relationship targets spelled as `targets`
    /// says, `type` and `scope` narrowed by `projection`, the Linked Entities
    /// `join` resolved nested into it.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Rendering`] when the entity cannot be
    /// written.
    pub(crate) fn render(
        &self,
        format: ResponseFormat,
        targets: TargetSpelling,
        join: Option<InlineJoin<'_>>,
        projection: StructuralProjection<'_>,
    ) -> Result<Value, NotificationError> {
        let notified = NotificationEntity::new(&self.entity, &self.stamped, format, targets).with_projection(projection);
        let notified = match self.changes() {
            Some(changes) => notified.with_changes(changes),
            None => notified,
        };
        let notified = match join {
            Some(join) => notified.with_inline_join(join),
            None => notified,
        };
        Ok(notified.render()?)
    }
}

/// `entity` with `stamped` written on it, in `format`, its Relationship
/// targets spelled as `targets` says.
///
/// # Errors
///
/// Returns [`NotificationError::Rendering`] when the entity cannot be written.
pub(crate) fn rendered_entity(entity: &Entity, stamped: &StampedInstants, format: ResponseFormat, targets: TargetSpelling) -> Result<Value, NotificationError> {
    Ok(NotificationEntity::new(entity, stamped, format, targets).render()?)
}
