//! Native PostgreSQL history writes inside the context transaction.

use andromeda_context_commit::{atomic_write::AtomicWrite, error::CommitError};
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use chrono::Utc;
use sqlx::{Postgres, Transaction};

/// Persist every landed write's native history and return projection work that
/// may be published only after the transaction commits.
pub(crate) async fn apply(
    recorder: &TemporalRecorder,
    tx: &mut Transaction<'_, Postgres>,
    landed: &[&AtomicWrite],
) -> Result<Vec<HistoryBatchEntry>, CommitError> {
    let recorded_at = Utc::now();
    let intents = landed
        .iter()
        .map(|write| TemporalAppendIntent::from_history(&write.entity_id, &write.history, recorded_at))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CommitError::Persistence { detail: error.to_string() })?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    recorder
        .record_intents_in_tx(tx, &intents)
        .await
        .map_err(|error| CommitError::Persistence { detail: error.to_string() })
}
