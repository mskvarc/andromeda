//! Writing typed temporal append intents in an existing anchor transaction.

use crate::{error::TemporalStoreError, intent_sink::PostgresTemporalHistorySink, repo::TemporalRepository, write_record_persist::persist_write_record};
use andromeda_context_commit::write_record::WriteRecord;
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
    pub const fn new(repo: Arc<TemporalRepository>) -> TemporalRecorder {
        TemporalRecorder { repo }
    }

    /// Record typed append intents inside the caller's anchor transaction.
    ///
    /// `record` is the write identity every intent in `intents` shares. The
    /// caller states it explicitly rather than this method inferring it from
    /// `intents.first()`, because nothing in the types otherwise enforces that
    /// every intent in the slice actually shares one record, and
    /// `write_records` has no foreign key from `attribute_history` or
    /// `scope_history` to catch a caller that got it wrong. It is persisted
    /// once rather than once per intent. Idempotency of the rows themselves is
    /// row-level: each entry insert carries `ON CONFLICT ... DO NOTHING` on its
    /// stream's write identity, so a commit retried after the transaction was
    /// already durable appends every intent again and lands nothing a second
    /// time, without this method needing to know which of the intents, if any,
    /// were already written.
    ///
    /// History only. The `current_attribute_values` projection is written by
    /// the same transaction from the same intents, but by the crate that owns
    /// the commit rather than from here: the projection is current entity
    /// state, and 0045 made Temporal Evolution independent of it in both
    /// directions.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails,
    /// [`TemporalStoreError::NotFound`] when an intent extends a Temporal
    /// Evolution that does not exist and declares no entity type to create it
    /// with, or [`TemporalStoreError::Validation`] when a row cannot be encoded.
    pub async fn record_intents_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        record: &WriteRecord,
        intents: &[TemporalAppendIntent],
    ) -> Result<(), TemporalStoreError> {
        if intents.is_empty() {
            return Ok(());
        }
        persist_write_record(tx, record).await?;
        PostgresTemporalHistorySink::append_entries_batch(&self.repo, tx, intents).await?;
        Ok(())
    }
}
