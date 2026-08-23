//! Native PostgreSQL history writes inside the context transaction.

use crate::commit_refusal::temporal_store;
use andromeda_context_commit::{error::CommitError, write_record::WriteRecord};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use sqlx::{Postgres, Transaction};

/// Persist the interpreted intents' native history in the caller's transaction.
pub(crate) async fn apply(
    recorder: &TemporalRecorder,
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    record: &WriteRecord,
    intents: &[TemporalAppendIntent],
) -> Result<(), CommitError> {
    recorder.record_intents_in_tx(tx, target, record, intents).await.map_err(temporal_store)
}
