//! Deferred side-effect jobs committed with the entity mutation.

use crate::{commit_refusal::context_store, consequences::ConsequenceWrite};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    error::CommitError,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_context_store::pending_side_effects::{
    payload::{EntityDocumentSharing, PersistedBatchSideEffectJob, PersistedChangeEvent, PersistedSysAttrs, PersistedTemporalEntity},
    repo::PendingSideEffectsRepository,
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// Persist asynchronous side-effect jobs for landed writes.
///
/// Every landed write of one commit contributes to a single queued job, which
/// is stored under `record`'s write identity: replaying the commit under its
/// original identity re-runs this enqueue against a row that already exists
/// and changes nothing. `base_ordinal` is stored alongside it, so a worker
/// that later leases and replays the job resumes the commit's ordinal counter
/// exactly where this commit left it, rather than restarting it at zero.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    landed: &[&AtomicWrite],
    record: &WriteRecord,
    base_ordinal: WriteOrdinal,
) -> Result<ConsequenceWrite, CommitError> {
    let merged = persisted_job(landed);
    if merged.temporal_entities.is_empty() && merged.change_events.is_empty() {
        return Ok(ConsequenceWrite::Nothing);
    }
    // Only the live branch has a side-effect queue, and queuing a Snapshot
    // commit's job there would replay it against the wrong branch. The entity
    // policy cannot produce such a job -- its `SideEffectPlacement` defers side
    // effects on the live branch only -- but this committer serves the
    // `ContextCommitter` port, which any caller may hand a job with any target,
    // so the port still refuses one it cannot honour.
    match target {
        ContextTarget::Live => {}
        ContextTarget::Snapshot(_) => {
            return Err(CommitError::Invariant {
                detail: "a Snapshot branch commit carries a deferred side-effect job; only the live branch defers side effects".to_owned(),
            });
        }
    }
    PendingSideEffectsRepository::enqueue_in_tx(tx, record, base_ordinal, &merged)
        .await
        .map_err(context_store)?;
    Ok(ConsequenceWrite::Written)
}

/// The one job every landed write of a commit contributes its deferred side
/// effects to.
///
/// The change events one write produces all carry the entity it left behind
/// in one shared allocation, so the job stores that entity once and every
/// later event refers back to the first one that stored it.
fn persisted_job(landed: &[&AtomicWrite]) -> PersistedBatchSideEffectJob {
    let mut merged = PersistedBatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: Vec::new(),
    };
    let mut documents = EntityDocumentSharing::default();
    for job in landed.iter().filter_map(|write| write.side_effects.as_ref()) {
        merged
            .temporal_entities
            .extend(job.temporal_entities.iter().map(|(id, expanded)| PersistedTemporalEntity {
                id: id.as_ref().clone(), // clone: the persisted DTO owns the validated entity URI.
                expanded: Value::clone(expanded),
            }));
        for event in &job.change_events {
            let position = merged.change_events.len();
            merged.change_events.push(persisted_event(event, position, &mut documents));
        }
    }
    merged
}

/// The persisted form of the change event at `position` in its job.
fn persisted_event<'a>(event: &'a ChangeEvent, position: usize, documents: &mut EntityDocumentSharing<'a>) -> PersistedChangeEvent {
    PersistedChangeEvent {
        entity_id: event.entity_id.clone(),       // clone: persistence owns the event identifier.
        entity_types: event.entity_types.clone(), // clone: persistence owns the event type list.
        trigger_reason: event.trigger_reason,
        changed_attributes: event.changed_attributes.clone(), // clone: persistence owns the changed-attribute list.
        changed_dataset_id: event.changed_dataset_id.clone(), // clone: persistence owns the instance discriminator.
        occurred_at: event.occurred_at,
        entity_snapshot: event.entity_snapshot.as_deref().cloned(),
        previous_attributes: event.previous_attributes.as_deref().cloned(),
        sys_attrs_snapshot: event.sys_attrs_snapshot.as_ref().map(|snapshot| PersistedSysAttrs {
            created_at: snapshot.created_at,
            modified_at: snapshot.modified_at,
            expires_at: snapshot.expires_at,
        }),
        origin: event.origin,
        current_payload: event.current_payload.as_ref().map(|document| documents.persist(position, document)),
    }
}

#[cfg(test)]
mod tests {
    use crate::consequences::side_effects::persisted_job;
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        batch_side_effect_job::BatchSideEffectJob,
        change_event::ChangeEvent,
        entity_mutation::EntityMutation,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use andromeda_postgresql_context_store::pending_side_effects::payload::PersistedEntityDocument;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a valid URI")
    }

    /// One write's changes, each carrying the entity the write left behind.
    fn write_with_changes(id: &str, triggers: &[NotificationTrigger]) -> AtomicWrite {
        let left_behind = Arc::new(json!([{"@id": id}]));
        let change_events = triggers
            .iter()
            .map(|trigger| {
                let mut event = ChangeEvent::new(uri(id), Vec::new(), *trigger);
                event.current_payload = Some(Arc::clone(&left_behind));
                event
            })
            .collect();
        AtomicWrite::builder()
            .entity_id(Arc::new(uri(id)))
            .entity(EntityMutation::None)
            .side_effects(Some(BatchSideEffectJob {
                temporal_entities: Vec::new(),
                change_events,
            }))
            .build()
    }

    /// A creation raises `entityCreated` and one `attributeCreated` per
    /// Attribute, all carrying the created entity: the job stores it once per
    /// write, and each write's own entity is stored for that write.
    #[test]
    fn the_entity_a_write_left_behind_is_stored_once_for_all_of_its_changes() {
        let first = write_with_changes(
            "urn:ngsi-ld:Building:1",
            &[NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated],
        );
        let second = write_with_changes(
            "urn:ngsi-ld:Building:2",
            &[NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated],
        );

        let job = persisted_job(&[&first, &second]);

        let documents: Vec<Option<PersistedEntityDocument>> = job.change_events.into_iter().map(|event| event.current_payload).collect();
        pretty_assertions::assert_eq!(
            documents,
            vec![
                Some(PersistedEntityDocument::Stored(json!([{"@id": "urn:ngsi-ld:Building:1"}]))),
                Some(PersistedEntityDocument::AsEvent(0)),
                Some(PersistedEntityDocument::Stored(json!([{"@id": "urn:ngsi-ld:Building:2"}]))),
                Some(PersistedEntityDocument::AsEvent(2)),
            ]
        );
    }

    /// A deletion carries no resulting entity, so nothing is stored for it.
    #[test]
    fn a_change_without_a_resulting_entity_stores_no_document() {
        let mut write = write_with_changes("urn:ngsi-ld:Building:1", &[NotificationTrigger::EntityDeleted]);
        if let Some(job) = write.side_effects.as_mut() {
            job.change_events.iter_mut().for_each(|event| event.current_payload = None::<Arc<Value>>);
        }
        let job = persisted_job(&[&write]);
        assert!(job.change_events.iter().all(|event| event.current_payload.is_none()));
    }
}
