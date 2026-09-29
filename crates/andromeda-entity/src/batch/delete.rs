//! Batch entity deletion (clause 5.6.10).
//!
//! A deleted entity's payload and system attributes are unreadable once the row
//! is gone, and the clause 5.6.10 notifications carry both. Inline mode takes
//! them from the delete itself, which reports what each row held. The
//! asynchronous mode cannot: its notifications have to be persisted in the same
//! transaction as the delete, which means they must exist before it runs, so it
//! reads them first.

use crate::{
    EntityService,
    error::EntityError,
    expanded_member::stored_entity_types,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    notification_plan::deletion,
    side_effect::SideEffectPlacement,
    sys_attrs::snapshot_with_sys_attrs,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    history_record::deletion_history,
    precondition::Precondition,
    subscription_match::SubscriptionMatch,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, SysAttrsSnapshot};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

/// One chunk's successfully deleted identifiers and its per-entity failures.
pub(super) type DeleteChunkOutcome = (Vec<Identifier>, Vec<(Identifier, EntityError)>);

impl EntityService {
    /// The durable notification records one deletion owes, mirroring the single
    /// delete (clause 5.6.6): one per subscription the deletion is owed to,
    /// carrying the pre-delete snapshot as its entity and previous image.
    ///
    /// The entity snapshot the durable match stores has the clause-4.8 system
    /// attributes embedded when they are known, because the deleted row is gone
    /// by delivery time and the durable path cannot re-read `createdAt`/
    /// `modifiedAt` the way a non-deletion notification does.
    fn delete_subscription_matches(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        payload: &Value,
        sys_attrs: Option<&SysAttrsSnapshot>,
    ) -> Result<Vec<SubscriptionMatch>, EntityError> {
        let entity_types = stored_entity_types(payload)?;
        let change = deletion(payload)?;
        let match_snapshot = sys_attrs.map_or_else(|| payload.clone(), |sys_attrs| snapshot_with_sys_attrs(payload, sys_attrs));
        let deleted = ChangedEntity {
            target,
            id,
            types: &entity_types,
        };
        let snapshots = MutationSnapshots {
            current: Some(&match_snapshot),
            previous: Some(payload),
        };
        Ok(self.notifier.subscription_matches(deleted, &change, snapshots))
    }

    pub(super) async fn batch_delete_chunk(&self, placement: SideEffectPlacement<'_>, chunk: &[Identifier]) -> Result<DeleteChunkOutcome, EntityError> {
        let target = placement.target();
        let _id_guards = self.acquire_id_locks(target, chunk).await;

        // Both paths need the payloads and the system attributes up front:
        // clause 5.6.6 records one tombstone per instance the entity still held,
        // and both the durable matches (on every path) and the asynchronous
        // path's persisted notifications carry the system attributes — all
        // unreadable once the row is gone.
        let payloads = self.reader.get_batch(target, chunk).await?;
        let sys_attrs = self.reader.get_sys_attrs_batch(target, chunk).await?;

        let mut writes = Vec::with_capacity(chunk.len());
        for id in chunk {
            let payload = payloads.get(id);
            // A durable-delivery broker commits the matches with the delete on
            // both paths. A live-delivery broker records none, so a `Prefer:
            // respond-async` delete defers its events to the job instead, for
            // whichever broker drains it to notify.
            let matches = match payload {
                Some(payload) => self.delete_subscription_matches(target, id, payload, sys_attrs.get(id))?,
                None => Vec::new(),
            };
            let side_effects = match (placement, payload) {
                (SideEffectPlacement::Inline(_), _) | (SideEffectPlacement::DeferredOnLive, None) => None,
                (SideEffectPlacement::DeferredOnLive, Some(payload)) => {
                    let change_events = self.notifier.owed_change_events(deletion_events(id, payload, sys_attrs.get(id))?);
                    (!change_events.is_empty()).then(|| BatchSideEffectJob {
                        temporal_entities: Vec::new(),
                        change_events,
                    })
                }
            };
            writes.push(
                AtomicWrite::builder()
                    .entity_id(Arc::new(id.clone())) // clone: each write owns a shared validated identifier.
                    .entity(EntityMutation::Delete)
                    .history(payload.map(deletion_history).transpose()?.unwrap_or_default())
                    .subscription_matches(matches)
                    .side_effects(side_effects)
                    .precondition(Precondition::MustExist)
                    .build(),
            );
        }

        let outcomes = self.writes.commit(target, writes).await?;
        if outcomes.len() != chunk.len() {
            return Err(EntityError::internal(format!(
                "delete chunk of {} entities produced {} outcomes",
                chunk.len(),
                outcomes.len()
            )));
        }

        let mut success = Vec::with_capacity(chunk.len());
        let mut errors = Vec::new();
        let mut events = Vec::new();
        for (id, outcome) in chunk.iter().zip(outcomes) {
            match outcome {
                WriteOutcome::Deleted(deleted) => {
                    if matches!(placement, SideEffectPlacement::Inline(_)) {
                        events.extend(inline_deletion_events(&deleted)?);
                    }
                    success.push(deleted.id);
                }
                WriteOutcome::NotFound
                | WriteOutcome::AlreadyExists
                | WriteOutcome::Superseded
                | WriteOutcome::Skipped
                | WriteOutcome::Created
                | WriteOutcome::Updated
                | WriteOutcome::NoEntityChange => {
                    errors.push((id.clone(), EntityError::not_found(id))); // clone: error result owns the entity ID
                }
            }
        }

        for event in events {
            self.notifier.publish(target, event);
        }

        Ok((success, errors))
    }
}

/// The clause 5.6.10 notification one deletion raises: one change carrying
/// the Entity's deletion and every instance it took with it (clause 5.8.6).
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the stored payload's types or
/// `datasetId`s cannot be read.
fn deletion_events(id: &Identifier, payload: &Value, sys_attrs: Option<&SysAttrsSnapshot>) -> Result<Vec<ChangeEvent>, EntityError> {
    Ok(vec![
        ChangeEvent::builder()
            .entity_id(id.clone()) // clone: the event owns its identifier
            .entity_types(stored_entity_types(payload)?)
            .change(deletion(payload)?)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .entity_snapshot(Some(payload.clone())) // clone: the event owns its snapshot
            .previous_attributes(Some(payload.clone())) // clone: the event owns its previous payload
            .sys_attrs_snapshot(sys_attrs.cloned())
            .build(),
    ])
}

/// The same events, read off what the delete reported the row held.
///
/// # Errors
///
/// Reports the [`EntityError::Internal`] of [`deletion_events`].
fn inline_deletion_events(deleted: &DeletedEntity) -> Result<Vec<ChangeEvent>, EntityError> {
    let sys_attrs = SysAttrsSnapshot {
        created_at: deleted.created_at,
        modified_at: deleted.modified_at,
        expires_at: deleted.expires_at,
    };
    deletion_events(&deleted.id, &deleted.payload, Some(&sys_attrs))
}
