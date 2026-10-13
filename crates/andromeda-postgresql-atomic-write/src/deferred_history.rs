//! Typed temporal intent creation inside the PostgreSQL anchor transaction.

use crate::{commit_refusal::temporal, intent_store::PostgresTemporalIntentStore};
use andromeda_context_commit::{error::CommitError, write_record::WriteRecord};
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_temporal::{append_intent::TemporalAppendIntent, error::TemporalError};
use sqlx::{Postgres, Transaction};

/// Enqueue the interpreted intents of the live-branch commit `record` for the
/// deferred history drain, each entry already in the shape of the history row
/// it becomes.
///
/// Only the live branch defers its history
/// ([`HistoryPlacement::Queued`](crate::history_mode::HistoryPlacement::Queued)),
/// so the intents always go to the live branch's queue.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    recorder: &TemporalRecorder,
    record: &WriteRecord,
    intents: &[TemporalAppendIntent],
) -> Result<(), CommitError> {
    if intents.is_empty() {
        return Ok(());
    }
    let records = recorder
        .intent_records_in_tx(tx, record, intents)
        .await
        .map_err(|error| temporal(TemporalError::from(error)))?;
    PostgresTemporalIntentStore::enqueue_in_tx(tx, record, records)
        .await
        .map_err(|error| temporal(error))
}
