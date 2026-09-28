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
    payload::{PersistedAttributeChange, PersistedBatchSideEffectJob, PersistedChangeEvent, PersistedSysAttrs, PersistedTemporalEntity},
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
fn persisted_job(landed: &[&AtomicWrite]) -> PersistedBatchSideEffectJob {
    let mut merged = PersistedBatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: Vec::new(),
    };
    for job in landed.iter().filter_map(|write| write.side_effects.as_ref()) {
        merged
            .temporal_entities
            .extend(job.temporal_entities.iter().map(|(id, expanded)| PersistedTemporalEntity {
                id: id.as_ref().clone(), // clone: the persisted DTO owns the validated entity URI.
                expanded: Value::clone(expanded),
            }));
        merged.change_events.extend(job.change_events.iter().map(persisted_event));
    }
    merged
}

/// The persisted form of one change event.
fn persisted_event(event: &ChangeEvent) -> PersistedChangeEvent {
    PersistedChangeEvent {
        entity_id: event.entity_id.clone(),       // clone: persistence owns the event identifier.
        entity_types: event.entity_types.clone(), // clone: persistence owns the event type list.
        lifecycle: event.change.lifecycle,
        instances: event
            .change
            .instances
            .iter()
            .map(|instance| PersistedAttributeChange {
                attribute: instance.key.attr.clone(),        // clone: persistence owns the attribute name.
                dataset_id: instance.key.dataset_id.clone(), // clone: persistence owns the instance discriminator.
                kind: instance.kind,
            })
            .collect(),
        occurred_at: event.occurred_at,
        entity_snapshot: event.entity_snapshot.as_deref().cloned(),
        previous_attributes: event.previous_attributes.as_deref().cloned(),
        sys_attrs_snapshot: event.sys_attrs_snapshot.as_ref().map(|snapshot| PersistedSysAttrs {
            created_at: snapshot.created_at,
            modified_at: snapshot.modified_at,
            expires_at: snapshot.expires_at,
        }),
        origin: event.origin,
        current_payload: event.current_payload.as_deref().cloned(),
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
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        expanded::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            instance_key::AttributeInstanceKey,
        },
        notification::ChangeOrigin,
    };
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Arc;

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a valid URI")
    }

    /// One write's change through `lifecycle`, carrying `current` as the entity
    /// the write left behind.
    fn write_with_change(id: &str, lifecycle: EntityLifecycle, current: Option<serde_json::Value>) -> AtomicWrite {
        let change = EntityChange::new(
            lifecycle,
            vec![AttributeChange {
                key: AttributeInstanceKey {
                    attr: "https://example.org/name".parse().expect("a valid attribute name"),
                    dataset_id: Some(uri("urn:dataset:a")),
                },
                kind: AttributeChangeKind::Created,
            }],
        );
        let event = ChangeEvent::builder()
            .entity_id(uri(id))
            .change(change)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .current_payload(current.map(Arc::new))
            .build();
        AtomicWrite::builder()
            .entity_id(Arc::new(uri(id)))
            .entity(EntityMutation::None)
            .side_effects(Some(BatchSideEffectJob {
                temporal_entities: Vec::new(),
                change_events: vec![event],
            }))
            .build()
    }

    /// Every landed write contributes its one change, in submission order,
    /// with its lifecycle, instances and the entity it left behind.
    #[test]
    fn every_write_of_a_commit_contributes_its_change_in_order() {
        let first = write_with_change(
            "urn:ngsi-ld:Building:1",
            EntityLifecycle::Created,
            Some(json!([{"@id": "urn:ngsi-ld:Building:1"}])),
        );
        let second = write_with_change(
            "urn:ngsi-ld:Building:2",
            EntityLifecycle::Modified,
            Some(json!([{"@id": "urn:ngsi-ld:Building:2"}])),
        );

        let job = persisted_job(&[&first, &second]);

        pretty_assertions::assert_eq!(job.change_events.len(), 2);
        pretty_assertions::assert_eq!(job.change_events[0].lifecycle, EntityLifecycle::Created);
        pretty_assertions::assert_eq!(job.change_events[1].lifecycle, EntityLifecycle::Modified);
        pretty_assertions::assert_eq!(job.change_events[0].instances[0].kind, AttributeChangeKind::Created);
        pretty_assertions::assert_eq!(job.change_events[0].instances[0].dataset_id.as_ref().map(Uri::as_str), Some("urn:dataset:a"));
        pretty_assertions::assert_eq!(job.change_events[1].current_payload, Some(json!([{"@id": "urn:ngsi-ld:Building:2"}])));
    }

    /// A deletion carries no resulting entity, so nothing is stored for it.
    #[test]
    fn a_change_without_a_resulting_entity_stores_no_document() {
        let write = write_with_change("urn:ngsi-ld:Building:1", EntityLifecycle::Deleted, None);
        let job = persisted_job(&[&write]);
        assert!(job.change_events.iter().all(|event| event.current_payload.is_none()));
    }
}
