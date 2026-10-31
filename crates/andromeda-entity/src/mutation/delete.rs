//! Delete Entity (clause 5.6.6).
//!
//! A deletion removes current state; its own effect and the tombstones its
//! Temporal Evolution records need only the entity's outline, its types, the
//! identity and family of each attribute instance it held, whether it held a
//! Scope (see [`crate::deletion_outline`]). The removed state is read only
//! when a subscription may be owed the deletion's notification, which renders
//! and filters the removed state (clauses 5.8.6 and 5.2.12). Whether one may be
//! is asked of the store every replica shares, and of this process's
//! subscription cache besides; a failed probe reads the state, so an
//! uncertain answer costs memory, never a notification.
//!
//! Either way the tombstones and the notification describe the state that was
//! read, so the deletion holds only while the entity is unchanged since: an
//! instance a concurrent write added would otherwise be removed without a
//! tombstone or a notification. A deletion that loses is outlined again from
//! the start, the watcher decision included.

use crate::{
    EntityService,
    contended_attempt::ContendedAttempt,
    deletion_outline::{DeletionOutline, DeletionWatchers},
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    notification_plan::deletion,
    snapshot_supply::SnapshotSupply,
    write_target::WriteTarget,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    history_record::{deletion_history, deletion_tombstones},
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::single,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::reads::attribute_change::deleted_instances;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
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
    /// Reports [`EntityError::NotFound`] when no entity carries the identifier
    /// of `entity` or the type selector of `entity` excludes it (clause
    /// 5.6.6.4), and [`EntityError::Persistence`] when the store could not read
    /// the entity or commit the deletion.
    pub async fn delete(&self, target: &ContextTarget, entity: WriteTarget<'_>) -> Result<(), EntityError> {
        let id = entity.id;
        let timer = entity::start(EntityOperation::Delete);
        let result: Result<(), EntityError> = async {
            self.retry_contended(async |_| match self.delete_attempt(target, entity).await? {
                AfterAttempt::Settled => Ok(()),
                AfterAttempt::Vanished => Err(ContendedAttempt::Failed(EntityError::not_found(id))),
                // A write landed between the read and the deletion: read the
                // entity again.
                AfterAttempt::Reread => Err(ContendedAttempt::Lost),
            })
            .await
            .map_err(|failure| failure.into_entity_error(id))
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// One attempt at the deletion: outline the entity, decide whether its
    /// removed state is needed, and delete it unless it changed since.
    async fn delete_attempt(&self, target: &ContextTarget, entity: WriteTarget<'_>) -> Result<AfterAttempt, EntityError> {
        let id = entity.id;
        let Some(outline) = self.deletion_outline.deletion_outline(target, id).await? else {
            return Ok(AfterAttempt::Vanished);
        };
        // Clause 5.6.6.4: an entity outside the request's type selector is not
        // the target, so it is as absent as one that does not exist.
        if !entity.selects(&outline.types) {
            return Ok(AfterAttempt::Vanished);
        }
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
            DeletionWatchers::Possible => self.delete_removed_state(target, entity).await,
            DeletionWatchers::None => self.delete_outline(target, id, outline, change).await,
        }
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
    async fn delete_outline(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        outline: DeletionOutline,
        change: EntityChange,
    ) -> Result<AfterAttempt, EntityError> {
        let DeletionOutline {
            types,
            instances,
            scope,
            version,
        } = outline;
        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the delete write owns its validated identifier.
            .entity(EntityMutation::Delete)
            // Clause 5.6.6 removes current state and leaves the Temporal
            // Evolution standing, so every instance the entity still held is
            // recorded as ended rather than being deleted with it.
            .history(deletion_tombstones(instances, scope))
            .precondition(Precondition::UnchangedSince(version))
            .build();
        let now = SystemInstant::new(Utc::now());
        let outcome = single(self.writes.commit(target, now, vec![write]).await?)?;
        let next = after_attempt(&outcome);
        let Some(committed_at) = outcome.changed_at() else {
            return Ok(next);
        };
        self.notifier
            .publish(
                target,
                committed_at,
                [ChangeEvent::builder()
                    .entity_id(id.clone()) // clone: the published event owns its identifier
                    .entity_types(types)
                    .change(change)
                    .occurred_at(now.get())
                    .origin(ChangeOrigin::Local)
                    .build()],
            )
            .await;
        Ok(next)
    }

    /// Delete an entity a subscription may be owed a notification for,
    /// reading the state it removes for the notification.
    async fn delete_removed_state(&self, target: &ContextTarget, entity: WriteTarget<'_>) -> Result<AfterAttempt, EntityError> {
        let id = entity.id;
        let Some(EntitySnapshot {
            state: removed,
            version,
            sys_attrs,
        }) = self.reader.get_snapshot(target, id).await?
        else {
            return Ok(AfterAttempt::Vanished);
        };
        if !entity.selects(removed.types().as_slice()) {
            return Ok(AfterAttempt::Vanished);
        }
        let entity_types = removed.types().as_slice().to_vec();

        let change = deletion(&removed);

        let deleted = ChangedEntity {
            target,
            id,
            types: &entity_types,
        };
        // The durable match stores the removed state as it is, and by delivery
        // time the deleted row is gone, so a match carries its clause-4.8
        // system attributes beside the state rather than re-reading them then
        // (the durable path re-derives only `deletedAt`, from the event
        // instant).
        let snapshots = MutationSnapshots {
            current: SnapshotSupply::Carried {
                state: &removed,
                sys_attrs: Some(&sys_attrs),
            },
            previous: Some(&removed),
        };
        // Clause 5.8.6: one deletion is one change, owed once to every
        // subscription it matches, through `entityDeleted` or through the
        // `attributeDeleted` of every instance the entity took with it.
        let subscription_matches = self.notifier.subscription_matches(deleted, &change, snapshots);

        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the delete write owns its validated identifier.
            .entity(EntityMutation::Delete)
            // Clause 5.6.6 removes current state and leaves the Temporal
            // Evolution standing, so every instance the entity still held is
            // recorded as ended rather than being deleted with it.
            .history(deletion_history(&removed))
            .subscription_matches(subscription_matches)
            .precondition(Precondition::UnchangedSince(version))
            .build();

        let now = SystemInstant::new(Utc::now());
        let outcome = single(self.writes.commit(target, now, vec![write]).await?)?;
        let next = after_attempt(&outcome);
        let Some(committed_at) = outcome.changed_at() else {
            return Ok(next);
        };

        self.notifier
            .publish(
                target,
                committed_at,
                [ChangeEvent::builder()
                    .entity_id(id.clone()) // clone: the published event owns its identifier
                    .entity_types(entity_types)
                    .change(change)
                    .occurred_at(now.get())
                    .origin(ChangeOrigin::Local)
                    .entity_snapshot(Some(Arc::clone(&removed)))
                    .previous_attributes(Some(removed))
                    .sys_attrs_snapshot(Some(sys_attrs))
                    .build()],
            )
            .await;

        Ok(next)
    }
}
