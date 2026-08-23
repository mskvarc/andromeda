//! Deferred side-effect jobs committed with the entity mutation.

use crate::commit_refusal::persistence;
use andromeda_context_commit::{atomic_write::AtomicWrite, error::CommitError, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_context_store::pending_side_effects::{
    payload::{PersistedBatchSideEffectJob, PersistedChangeEvent, PersistedSysAttrs, PersistedTemporalEntity},
    repo::PendingSideEffectsRepository,
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// Persist asynchronous side-effect jobs for landed writes.
///
/// Every landed write of one commit contributes to a single queued job, which
/// is stored under `record`'s write identity: replaying the commit under its
/// original identity re-runs this enqueue against a row that already exists
/// and changes nothing. `consequence_base` is stored alongside it, so a worker
/// that later leases and replays the job resumes the commit's ordinal counter
/// exactly where this commit left it, rather than restarting it at zero.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    landed: &[&AtomicWrite],
    record: &WriteRecord,
    consequence_base: WriteOrdinal,
) -> Result<(), CommitError> {
    let mut merged = PersistedBatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: Vec::new(),
    };
    for write in landed {
        let Some(job) = &write.side_effects else {
            continue;
        };
        merged
            .temporal_entities
            .extend(job.temporal_entities.iter().map(|(id, expanded)| PersistedTemporalEntity {
                id: id.as_ref().clone(), // clone: the persisted DTO owns the validated entity URI.
                expanded: Value::clone(expanded),
            }));
        merged.change_events.extend(job.change_events.iter().map(|event| PersistedChangeEvent {
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
        }));
    }
    if merged.temporal_entities.is_empty() && merged.change_events.is_empty() {
        return Ok(());
    }
    PendingSideEffectsRepository::enqueue_in_tx(tx, target, record, consequence_base, &merged)
        .await
        .map_err(persistence)?;
    Ok(())
}
