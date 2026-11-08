//! Deferred side-effect jobs committed with the entity mutation.

use crate::{
    commit_refusal::{context_store, name_lookup},
    consequences::ConsequenceWrite,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    commit_invariant::CommitInvariant,
    error::CommitError,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_attribute_instance::{name_keys::MentionedNames, name_resolution::resolve_mentioned};
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    pending_side_effects::{
        change_event_record::{mention_event, record_of_event},
        repo::PendingSideEffectsRepository,
    },
};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use sqlx::{Postgres, Transaction};

/// Persist the change events landed writes still owe as one side-effect job.
///
/// Every landed write of one commit contributes to a single queued job, which
/// is stored under `record`'s write identity: replaying the commit under its
/// original identity re-runs this enqueue against a row that already exists
/// and changes nothing. `base_ordinal` is stored alongside it, so a worker
/// that later leases and replays the job resumes the commit's ordinal counter
/// exactly where this commit left it, rather than restarting it at zero. A
/// commit that owes no change queues no job.
///
/// Each event's states are stored as `entity_state_record`s, every name a
/// key minted in this transaction, one resolution per dictionary for the
/// whole job.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    landed: &[&AtomicWrite],
    record: &WriteRecord,
    base_ordinal: WriteOrdinal,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<ConsequenceWrite, CommitError> {
    let events = owed_events(landed);
    if events.is_empty() {
        return Ok(ConsequenceWrite::Nothing);
    }
    // Only the live branch has a side-effect queue, and queuing a Snapshot
    // commit's job there would replay it against the wrong branch. The entity
    // policy cannot produce such a job (its `SideEffectPlacement` defers side
    // effects on the live branch only), but this committer serves the
    // `ContextCommitter` port, which any caller may hand a job with any target,
    // so the port still refuses one it cannot honour.
    match target {
        ContextTarget::Live => {}
        ContextTarget::Snapshot(_) => {
            return Err(CommitInvariant::SnapshotSideEffectJob.into());
        }
    }
    let mut mentioned = MentionedNames::default();
    for event in &events {
        mention_event(event, &mut mentioned);
    }
    let keys = resolve_mentioned(tx, mentioned, entity_type_names, attribute_names)
        .await
        .map_err(|error| name_lookup(&error))?;
    let records = events
        .iter()
        .map(|event| record_of_event(event, &keys))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| context_store(ContextStoreError::from(error)))?;
    PendingSideEffectsRepository::enqueue_in_tx(tx, record, base_ordinal, records)
        .await
        .map_err(|error| context_store(error))?;
    Ok(ConsequenceWrite::Written)
}

/// Every change event the landed writes' jobs owe, in submission order.
fn owed_events<'a>(landed: &[&'a AtomicWrite]) -> Vec<&'a ChangeEvent> {
    landed
        .iter()
        .filter_map(|write| write.side_effects.as_ref())
        .flat_map(|job| job.change_events.iter())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::consequences::side_effects::owed_events;
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        batch_side_effect_job::BatchSideEffectJob,
        change_event::ChangeEvent,
        entity_mutation::EntityMutation,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
    };
    use chrono::Utc;
    use std::sync::Arc;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a valid URI")
    }

    /// One write whose job owes its change through `lifecycle`.
    fn write_with_change(id: &str, lifecycle: EntityLifecycle) -> AtomicWrite {
        let event = ChangeEvent::builder()
            .entity_id(uri(id))
            .change(EntityChange::new(lifecycle, Vec::new()))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build();
        AtomicWrite::builder()
            .entity_id(Arc::new(uri(id)))
            .entity(EntityMutation::None)
            .side_effects(Some(BatchSideEffectJob { change_events: vec![event] }))
            .build()
    }

    /// Every landed write contributes its one change, in submission order.
    #[test]
    fn every_write_of_a_commit_contributes_its_change_in_order() {
        let first = write_with_change("urn:ngsi-ld:Building:1", EntityLifecycle::Created);
        let second = write_with_change("urn:ngsi-ld:Building:2", EntityLifecycle::Modified);
        let quiet = AtomicWrite::builder()
            .entity_id(Arc::new(uri("urn:ngsi-ld:Building:3")))
            .entity(EntityMutation::None)
            .build();

        let events = owed_events(&[&first, &quiet, &second]);

        let owed: Vec<(&str, EntityLifecycle)> = events.iter().map(|event| (event.entity_id.as_str(), event.change.lifecycle)).collect();
        pretty_assertions::assert_eq!(
            owed,
            vec![
                ("urn:ngsi-ld:Building:1", EntityLifecycle::Created),
                ("urn:ngsi-ld:Building:2", EntityLifecycle::Modified)
            ]
        );
    }
}
