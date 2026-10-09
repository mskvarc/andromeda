//! Writing typed temporal append intents in an existing anchor transaction.

use crate::{error::TemporalStoreError, intent_sink::PostgresTemporalHistorySink, repo::TemporalRepository, write_record_persist::persist_write_record};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;

/// Records typed temporal intents in the caller's PostgreSQL transaction.
#[derive(Debug)]
pub struct TemporalRecorder {
    /// Repository whose statements the recorder runs.
    repo: Arc<TemporalRepository>,
}

impl TemporalRecorder {
    /// Create a temporal recorder backed by the given repository.
    #[must_use]
    pub const fn new(repo: Arc<TemporalRepository>) -> TemporalRecorder {
        TemporalRecorder { repo }
    }

    /// Record typed append intents inside the caller's anchor transaction.
    ///
    /// `record` is the write identity every intent in `intents` shares, persisted once rather than once per intent. The caller states it explicitly rather than this method inferring it from `intents.first()`, and every intent is checked against it before anything is written: history rows are stamped from each intent's own record, `write_records` has no foreign key from `attribute_history` or `scope_history` to catch a disagreement, and the release of a write record seeks its history rows by the persisted `recorded_at`, so a row stamped with another instant would protect nothing and survive its record.
    ///
    /// Idempotency of the rows themselves is row-level: each entry insert carries `ON CONFLICT ... DO NOTHING` on its stream's write identity, so a commit retried after the transaction was already durable appends every intent again and lands nothing a second time.
    ///
    /// History only. Current entity state is written by the same transaction, but by the crate that owns the commit rather than from here, and the temporal catalogue keeps Temporal Evolution independent of it in both directions.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::WriteRecordMismatch`] when an intent
    /// carries a write record other than `record`,
    /// [`TemporalStoreError::Database`] when a statement fails,
    /// [`TemporalStoreError::NotFound`] when an intent records an observation
    /// against a Temporal Evolution that does not exist and declares no entity
    /// type to create it with, or [`TemporalStoreError::Validation`] when a row
    /// cannot be encoded.
    pub async fn record_intents_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        record: &WriteRecord,
        intents: &[TemporalAppendIntent],
    ) -> Result<(), TemporalStoreError> {
        if intents.is_empty() {
            return Ok(());
        }
        refuse_foreign_records(record, intents)?;
        persist_write_record(tx, record).await?;
        PostgresTemporalHistorySink::append_entries_batch(&self.repo, tx, target, intents).await?;
        Ok(())
    }
}

/// Refuse a batch in which any intent carries a write record other than the
/// one its commit persists.
///
/// # Errors
///
/// Returns [`TemporalStoreError::WriteRecordMismatch`] naming the first intent
/// whose record differs from `record`.
fn refuse_foreign_records(record: &WriteRecord, intents: &[TemporalAppendIntent]) -> Result<(), TemporalStoreError> {
    match intents.iter().find(|intent| intent.record != *record) {
        Some(intent) => Err(TemporalStoreError::WriteRecordMismatch {
            write_id: intent.record.write_id(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::TemporalStoreError, recorder::refuse_foreign_records};
    use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use andromeda_temporal::append_intent::TemporalAppendIntent;
    use chrono::{TimeZone, Utc};

    /// An intent for one entity, stamped with `record`.
    fn intent(record: WriteRecord) -> TemporalAppendIntent {
        TemporalAppendIntent {
            record,
            base_ordinal: WriteOrdinal::at(0),
            entity_id: "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"),
            entity_types: Vec::new(),
            entries: Vec::new(),
            selection_value: None,
        }
    }

    #[test]
    fn an_intent_stamped_with_a_different_write_record_is_refused() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let persisted = WriteRecord::mint(instant);
        let foreign = WriteRecord::mint(instant);
        assert!(refuse_foreign_records(&persisted, &[intent(persisted)]).is_ok());
        let refused = refuse_foreign_records(&persisted, &[intent(persisted), intent(foreign)]);
        assert!(
            matches!(refused, Err(TemporalStoreError::WriteRecordMismatch { write_id }) if write_id == foreign.write_id()),
            "{refused:?}"
        );
    }
}
