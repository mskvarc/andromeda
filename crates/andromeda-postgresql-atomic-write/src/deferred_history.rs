//! Typed temporal intent creation inside the PostgreSQL anchor transaction.

use crate::intent_store::PostgresTemporalIntentStore;
use andromeda_context_commit::{atomic_write::AtomicWrite, error::CommitError};
use andromeda_temporal::append_intent::TemporalAppendIntent;
use chrono::Utc;
use sqlx::{Postgres, Transaction};

/// Persist one ordered intent per landed write that carries temporal consequences.
pub(crate) async fn apply(tx: &mut Transaction<'_, Postgres>, landed: &[&AtomicWrite]) -> Result<usize, CommitError> {
    let recorded_at = Utc::now();
    let intents = landed
        .iter()
        .map(|write| TemporalAppendIntent::from_history(&write.entity_id, &write.history, recorded_at))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CommitError::Persistence { detail: error.to_string() })?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    PostgresTemporalIntentStore::enqueue_in_tx(tx, &intents)
        .await
        .map_err(|error| CommitError::Persistence { detail: error.to_string() })?;
    Ok(intents.len())
}
