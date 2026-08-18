//! Batch entity deletion (clause 5.6.10).
//!
//! A deleted entity's payload and system attributes are unreadable once the row
//! is gone, and the clause 5.6.10 notifications carry both. Inline mode takes
//! them from the delete itself, which reports what each row held. The
//! asynchronous mode cannot: its notifications have to be persisted in the same
//! transaction as the delete, which means they must exist before it runs, so it
//! reads them first.

use crate::{EntityService, error::EntityError, side_effect::BatchSideEffectMode, sys_attrs::snapshot_with_sys_attrs};
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
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    expanded::entity_member::{extract_attribute_names, extract_types},
    notification::{ChangeOrigin, NotificationTrigger, SysAttrsSnapshot},
};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

/// One chunk's successfully deleted identifiers and its per-entity failures.
pub(super) type DeleteChunkOutcome = (Vec<Uri>, Vec<(Uri, EntityError)>);

impl EntityService {
    /// The durable notification records one deletion owes, mirroring the single
    /// delete (clause 5.6.6): one `attributeDeleted` naming every attribute the
    /// entity took with it, then one `entityDeleted`. Both carry the pre-delete
    /// snapshot as their entity and previous image.
    ///
    /// The entity snapshot the durable match stores has the clause-4.8 system
    /// attributes embedded when they are known, because the deleted row is gone
    /// by delivery time and the durable path cannot re-read `createdAt`/
    /// `modifiedAt` the way a non-deletion notification does.
    fn delete_subscription_matches(&self, id: &Uri, payload: &Value, sys_attrs: Option<&SysAttrsSnapshot>) -> Vec<SubscriptionMatch> {
        let entity_types = extract_types(payload);
        let attr_keys = extract_attribute_names(payload);
        let match_snapshot = sys_attrs.map_or_else(|| payload.clone(), |sys_attrs| snapshot_with_sys_attrs(payload, sys_attrs));
        let mut matches = Vec::new();
        if !attr_keys.is_empty() {
            matches.extend(self.build_subscription_matches(
                id,
                &entity_types,
                NotificationTrigger::AttributeDeleted,
                &attr_keys,
                Some(&match_snapshot),
                Some(payload),
            ));
        }
        matches.extend(self.build_subscription_matches(id, &entity_types, NotificationTrigger::EntityDeleted, &[], Some(&match_snapshot), Some(payload)));
        matches
    }

    pub(super) async fn batch_delete_chunk(&self, chunk: &[Uri], mode: BatchSideEffectMode) -> Result<DeleteChunkOutcome, EntityError> {
        let _id_guards = self.acquire_id_locks(chunk).await;

        // Both paths need the payloads and the system attributes up front:
        // clause 5.6.6 records one tombstone per instance the entity still held,
        // and both the durable matches (on every path) and the asynchronous
        // path's persisted notifications carry the system attributes — all
        // unreadable once the row is gone.
        let payloads = self.reader.get_batch(chunk).await?;
        let sys_attrs = self.reader.get_sys_attrs_batch(chunk).await?;

        let mut writes = Vec::with_capacity(chunk.len());
        for id in chunk {
            let payload = payloads.get(id);
            // The durable matches commit with the delete on both paths — the
            // only notification channel a `Prefer: respond-async` batch delete
            // has, since its deferred job carries the in-process events only.
            let matches = payload
                .map(|payload| self.delete_subscription_matches(id, payload, sys_attrs.get(id)))
                .unwrap_or_default();
            let side_effects = match mode {
                BatchSideEffectMode::Inline => None,
                BatchSideEffectMode::Enqueue => payload.map(|payload| BatchSideEffectJob {
                    temporal_entities: Vec::new(),
                    change_events: deletion_events(id, payload, sys_attrs.get(id)),
                }),
            };
            writes.push(
                AtomicWrite::builder()
                    .entity_id(Arc::new(id.clone())) // clone: each write owns a shared validated identifier.
                    .entity(EntityMutation::Delete)
                    .history(payload.map(deletion_history).unwrap_or_default())
                    .subscription_matches(matches)
                    .side_effects(side_effects)
                    .precondition(Precondition::MustExist)
                    .build(),
            );
        }

        let outcomes = self.writes.commit(writes).await?;
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
                    if matches!(mode, BatchSideEffectMode::Inline) {
                        events.extend(inline_deletion_events(&deleted));
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

        self.apply_inline_side_effects(BatchSideEffectJob {
            temporal_entities: Vec::new(),
            change_events: events,
        });

        Ok((success, errors))
    }
}

/// The clause 5.6.10 notifications one deletion raises.
///
/// Two events, not one: an `attributeDeleted` subscription has to hear about
/// every attribute the entity took with it.
fn deletion_events(id: &Uri, payload: &Value, sys_attrs: Option<&SysAttrsSnapshot>) -> Vec<ChangeEvent> {
    let entity_types = extract_types(payload);
    let attr_keys = extract_attribute_names(payload);

    let mut events = Vec::with_capacity(2);
    if !attr_keys.is_empty() {
        events.push(
            ChangeEvent::builder()
                .entity_id(id.clone()) // clone: the event owns its identifier
                .entity_types(entity_types.clone()) // clone: value also needed by the EntityDeleted event
                .trigger_reason(NotificationTrigger::AttributeDeleted)
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .changed_attributes(attr_keys)
                .entity_snapshot(Some(payload.clone())) // clone: the event owns its snapshot
                .previous_attributes(Some(payload.clone())) // clone: the event owns its previous payload
                .sys_attrs_snapshot(sys_attrs.cloned())
                .build(),
        );
    }
    events.push(
        ChangeEvent::builder()
            .entity_id(id.clone()) // clone: the event owns its identifier
            .entity_types(entity_types)
            .trigger_reason(NotificationTrigger::EntityDeleted)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .entity_snapshot(Some(payload.clone())) // clone: the event owns its snapshot
            .previous_attributes(Some(payload.clone())) // clone: the event owns its previous payload
            .sys_attrs_snapshot(sys_attrs.cloned())
            .build(),
    );
    events
}

/// The same events, read off what the delete reported the row held.
fn inline_deletion_events(deleted: &DeletedEntity) -> Vec<ChangeEvent> {
    let sys_attrs = SysAttrsSnapshot {
        created_at: deleted.created_at,
        modified_at: deleted.modified_at,
        expires_at: deleted.expires_at,
    };
    deletion_events(&deleted.id, &deleted.payload, Some(&sys_attrs))
}
