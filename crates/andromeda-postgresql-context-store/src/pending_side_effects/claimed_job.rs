//! One claimed side-effect job read back from its row.
//!
//! A job's events store every name as a dictionary key, so a claimed row is
//! first validated with its keys, then the names behind every key the job
//! holds are looked up once per dictionary, and only then is the job
//! assembled.

use crate::{
    pending_side_effects::change_event_record::{PendingChangeEvent, ReadChangeEvent},
    store_fault::undecodable,
};
use andromeda_context_commit::{
    batch_side_effect_job::BatchSideEffectJob,
    side_effect_job_id::SideEffectJobId,
    store_instant::StoreInstant,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_entity::{error::EntityError, side_effect_queue::LeasedSideEffect};
use andromeda_postgresql_attribute_instance::name_keys::{HeldKeys, KeyNames};
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};

/// One claimed row, every stored value validated but the names behind its
/// keys.
pub(crate) struct ReadJob {
    /// Queue entry identifier.
    id: SideEffectJobId,
    /// Attempts already made.
    attempts: i32,
    /// The enqueueing commit's identity and instant.
    record: WriteRecord,
    /// First ordinal free in the enqueueing commit's numbering.
    base_ordinal: WriteOrdinal,
    /// The enqueueing transaction's timestamp, which `created_at` defaults to.
    committed_at: StoreInstant,
    /// The owed changes, in the order the commit made them.
    events: Vec<ReadChangeEvent>,
}

impl ReadJob {
    /// Every dictionary key the job's events hold, each once.
    pub(crate) fn held_keys(&self) -> HeldKeys {
        let mut held = HeldKeys::default();
        for event in &self.events {
            event.collect_keys(&mut held);
        }
        held.distinct()
    }

    /// The leased job, every key read back through `names`.
    pub(crate) fn assemble(self, names: &KeyNames) -> Result<LeasedSideEffect, EntityError> {
        let change_events = self
            .events
            .into_iter()
            .map(|event| event.assemble(names).map_err(|error| EntityError::from(undecodable(&error))))
            .collect::<Result<_, _>>()?;
        Ok(LeasedSideEffect {
            id: self.id,
            attempts: self.attempts,
            job: BatchSideEffectJob { change_events },
            record: self.record,
            base_ordinal: self.base_ordinal,
            committed_at: self.committed_at,
        })
    }
}

/// Validate one claimed row but the names behind its keys.
pub(crate) fn read_row(id: SideEffectJobId, row: &PgRow) -> Result<ReadJob, EntityError> {
    let write_id = WriteId::from_uuid(stored_column(row, "write_id")?);
    let recorded_at: DateTime<Utc> = stored_column(row, "recorded_at")?;
    let stored_base: i32 = stored_column(row, "base_ordinal")?;
    Ok(ReadJob {
        id,
        attempts: stored_column(row, "attempt_count")?,
        record: WriteRecord::restored(write_id, recorded_at),
        base_ordinal: WriteOrdinal::try_from(stored_base).map_err(|error| EntityError::from(undecodable(&error)))?,
        committed_at: StoreInstant::new(stored_column(row, "created_at")?),
        events: read_events(stored_column(row, "events")?)?,
    })
}

/// Validate every stored event of one job but the names behind its keys.
fn read_events(records: Vec<PendingChangeEvent>) -> Result<Vec<ReadChangeEvent>, EntityError> {
    records
        .into_iter()
        .map(|record| ReadChangeEvent::try_from(record).map_err(|error| EntityError::from(undecodable(&error))))
        .collect()
}

/// Read one column of a claimed row; a column that does not decode is an
/// undecodable record.
pub(crate) fn stored_column<'r, T>(row: &'r PgRow, column: &str) -> Result<T, EntityError>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column).map_err(|error| EntityError::from(undecodable(&error)))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        current_attribute::round_trip_tests::dictionary_of,
        entity_state_record::state_record::record_of,
        notification_delivery::claimed_event::tests::temperature,
        pending_side_effects::{change_event_record::PendingChangeEvent, claimed_job::read_events},
    };
    use andromeda_entity::error::EntityError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use chrono::Utc;

    /// A job whose stored state no binary can read is refused as undecodable,
    /// which the claim answers by dead-lettering the job in place of
    /// returning it.
    #[test]
    fn an_unreadable_job_is_dead_lettered() {
        let state = temperature(20);
        let (keys, _) = dictionary_of([&state]);
        let mut snapshot = record_of(&state, &keys).expect("a committed state has a record");
        if let Some(instance) = snapshot.instances.first_mut() {
            instance.value_string = Some("twenty".to_owned());
        }
        let event = PendingChangeEvent {
            lifecycle: 2,
            entity_id: "urn:ngsi-ld:Building:1".to_owned(),
            entity_type_name_keys: snapshot.entity_type_name_keys.clone(), // clone: the event and its state each hold the type keys.
            change_kinds: Vec::new(),
            change_attribute_name_keys: Vec::new(),
            change_dataset_ids: Vec::new(),
            occurred_at: Utc::now(),
            origin: 1,
            entity_snapshot: Some(snapshot),
            previous_snapshot: None,
            entity_created_at: None,
            entity_modified_at: None,
            entity_expires_at: None,
        };
        assert!(matches!(
            read_events(vec![event]),
            Err(EntityError::Persistence(PersistenceFailure::Undecodable))
        ));
    }
}
