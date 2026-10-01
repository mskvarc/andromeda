//! Delete Entity (clause 5.6.6).
//!
//! A deletion removes current state; its own effect and the tombstones its
//! Temporal Evolution records need only the entity's outline -- its types, the
//! identity and family of each attribute instance it held, whether it held a
//! Scope (see [`crate::deletion_outline`]). The removed payload is read only
//! when a subscription may be owed the deletion's notification, which renders
//! and filters the removed state (clauses 5.8.6 and 5.2.12). Whether one may be
//! is asked of the store every replica shares, and of this process's
//! subscription cache besides; a failed probe reads the payload, so an
//! uncertain answer costs memory, never a notification.

use crate::{
    EntityService,
    deletion_outline::{DeletionOutline, DeletionWatchers},
    error::EntityError,
    expanded_member::stored_entity_types,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    notification_plan::deletion,
    snapshot_supply::SnapshotSupply,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    history_record::{deletion_history, deletion_tombstones},
    precondition::Precondition,
    write_outcome::single,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    expanded::attribute_change::deleted_instances,
    notification::ChangeOrigin,
};
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use std::sync::Arc;

impl EntityService {
    /// Delete an entity by its URI (NGSI-LD 5.6.6).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::NotFound`] when no entity carries `id`, and
    /// [`EntityError::Persistence`] when the store could not read the entity or
    /// commit the deletion.
    pub async fn delete(&self, target: &ContextTarget, id: &Identifier) -> Result<(), EntityError> {
        let timer = entity::start(EntityOperation::Delete);
        let result: Result<(), EntityError> = async {
            let Some(outline) = self.deletion_outline.deletion_outline(target, id).await? else {
                return Err(EntityError::not_found(id));
            };
            let change = EntityChange::new(
                EntityLifecycle::Deleted,
                deleted_instances(outline.instances.iter().map(|(key, _)| key.clone())),
            ); // clone: the change owns the identities the outline lists
            let deleted = ChangedEntity {
                target,
                id,
                types: &outline.types,
            };
            match self.deletion_watchers(deleted, &change).await {
                DeletionWatchers::Possible => self.delete_removed_state(target, id).await,
                DeletionWatchers::None => self.delete_outline(target, id, outline, change).await,
            }
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Whether a subscription may be owed the deletion `change` describes,
    /// asked of the shared store and of this process's cache.
    async fn deletion_watchers(&self, deleted: ChangedEntity<'_>, change: &EntityChange) -> DeletionWatchers {
        match self.deletion_outline.deletion_watchers(deleted.target).await {
            Ok(DeletionWatchers::None) => self.notifier.cached_deletion_watchers(deleted, change),
            Ok(DeletionWatchers::Possible) => DeletionWatchers::Possible,
            Err(error) => {
                tracing::warn!(entity = %deleted.id, %error, "the deletion-watcher probe failed; the deletion reads the removed state");
                DeletionWatchers::Possible
            }
        }
    }

    /// Delete an entity no subscription can be owed a notification for, from
    /// its outline alone.
    async fn delete_outline(&self, target: &ContextTarget, id: &Identifier, outline: DeletionOutline, change: EntityChange) -> Result<(), EntityError> {
        let DeletionOutline { types, instances, scope } = outline;
        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the delete write owns its validated identifier.
            .entity(EntityMutation::Delete)
            // Clause 5.6.6 removes current state and leaves the Temporal
            // Evolution standing, so every instance the entity still held is
            // recorded as ended rather than being deleted with it.
            .history(deletion_tombstones(instances, scope))
            .precondition(Precondition::MustExist)
            .build();
        if single(self.writes.commit(target, vec![write]).await?)?.is_not_found() {
            return Err(EntityError::not_found(id));
        }
        self.notifier.publish(
            target,
            ChangeEvent::builder()
                .entity_id(id.clone()) // clone: the published event owns its identifier
                .entity_types(types)
                .change(change)
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .build(),
        );
        Ok(())
    }

    /// Delete an entity a subscription may be owed a notification for,
    /// reading the state it removes for the notification.
    async fn delete_removed_state(&self, target: &ContextTarget, id: &Identifier) -> Result<(), EntityError> {
        let removed = Arc::new(self.reader.get(target, id).await?.ok_or_else(|| EntityError::not_found(id))?);
        let entity_types = stored_entity_types(&removed)?;

        // Captured before the delete so the notification can carry them: once
        // the row is gone they are unreadable.
        let sys_attrs_snapshot = match self.reader.get_sys_attrs(target, id).await {
            Ok(attrs) => attrs,
            Err(e) => {
                tracing::warn!("Failed to fetch sys_attrs before delete for {}: {}", id, e);
                None
            }
        };

        let change = deletion(&removed)?;

        let deleted = ChangedEntity {
            target,
            id,
            types: &entity_types,
        };
        // The durable match stores the removed state verbatim, and by delivery
        // time the deleted row is gone, so a match is notified with its
        // clause-4.8 system attributes embedded rather than re-read then (the
        // durable path re-derives only `deletedAt`, from the event instant).
        // The enriched copy is built only once a subscription matched.
        let snapshots = MutationSnapshots {
            current: SnapshotSupply::Removed {
                removed: &removed,
                sys_attrs: sys_attrs_snapshot.as_ref(),
            },
            previous: Some(&removed),
        };
        // Clause 5.8.6: one deletion is one change, owed once to every
        // subscription it matches -- through `entityDeleted` or through the
        // `attributeDeleted` of every instance the entity took with it.
        let subscription_matches = self.notifier.subscription_matches(deleted, &change, snapshots);

        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the delete write owns its validated identifier.
            .entity(EntityMutation::Delete)
            // Clause 5.6.6 removes current state and leaves the Temporal
            // Evolution standing, so every instance the entity still held is
            // recorded as ended rather than being deleted with it.
            .history(deletion_history(&removed)?)
            .subscription_matches(subscription_matches)
            .precondition(Precondition::MustExist)
            .build();

        if single(self.writes.commit(target, vec![write]).await?)?.is_not_found() {
            return Err(EntityError::not_found(id));
        }

        self.notifier.publish(
            target,
            ChangeEvent::builder()
                .entity_id(id.clone()) // clone: the published event owns its identifier
                .entity_types(entity_types)
                .change(change)
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .entity_snapshot(Some(Arc::clone(&removed)))
                .previous_attributes(Some(removed))
                .sys_attrs_snapshot(sys_attrs_snapshot)
                .build(),
        );

        Ok(())
    }
}
