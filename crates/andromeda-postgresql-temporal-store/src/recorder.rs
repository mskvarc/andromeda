//! Writing typed temporal append intents in an existing anchor transaction.

use crate::{error::TemporalStoreError, intent_sink::PostgresTemporalHistorySink, repo::TemporalRepository};
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;

/// Records typed temporal intents in the caller's PostgreSQL transaction.
#[derive(Debug)]
pub struct TemporalRecorder {
    repo: Arc<TemporalRepository>,
}

impl TemporalRecorder {
    /// Create a temporal recorder backed by the given repository.
    pub const fn new(repo: Arc<TemporalRepository>) -> TemporalRecorder {
        TemporalRecorder { repo }
    }

    /// Record typed append intents inside the caller's anchor transaction.
    pub async fn record_intents_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        intents: &[TemporalAppendIntent],
    ) -> Result<Vec<HistoryBatchEntry>, TemporalStoreError> {
        let mut history = Vec::new();
        for intent in intents {
            history.extend(PostgresTemporalHistorySink::append_entries(&self.repo, tx, intent).await?);
        }
        Ok(history)
    }
}
