//! Typed temporal intent creation inside the PostgreSQL anchor transaction.

use crate::{commit_refusal::temporal, intent_store::PostgresTemporalIntentStore};
use andromeda_context_commit::error::CommitError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use sqlx::{Postgres, Transaction};

/// Enqueue the interpreted intents for the deferred history drain.
pub(crate) async fn apply(tx: &mut Transaction<'_, Postgres>, target: &ContextTarget, intents: &[TemporalAppendIntent]) -> Result<(), CommitError> {
    PostgresTemporalIntentStore::enqueue_in_tx(tx, target, intents).await.map_err(temporal)
}
