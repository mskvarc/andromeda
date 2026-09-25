//! PostgreSQL anchor persistence and delivery state for temporal append intents.
//!
//! The store's enqueue, run inside the entity anchor transaction, lives in
//! [`enqueue`]; the claim, pending-read and lease-guarded statements in
//! [`lease_statements`]; the port implementations here.

/// Enqueueing temporal append intents inside the entity anchor transaction.
mod enqueue;
/// The claim, pending-read and lease-guarded acknowledgement statements.
mod lease_statements;

use crate::intent_store::lease_statements::{acknowledge_statement, claim_statement, pending_statement, retry_statement};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use andromeda_temporal::{
    append_intent::{SequencedTemporalAppendIntent, TemporalIntentSequence},
    error::TemporalError,
    port::{
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::PostgresQueryBuilder;
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow};
use std::fmt::Display;

/// The statement name the intent claim is observed under.
const INTENT_CLAIM: StatementName = StatementName("temporal-intent-claim");
/// The statement name intent acknowledgement and retry are observed under.
const INTENT_ACKNOWLEDGE: StatementName = StatementName("temporal-intent-acknowledge");

/// PostgreSQL-backed pending temporal intent queue for one tenant.
#[derive(Clone, Debug)]
pub struct PostgresTemporalIntentStore {
    /// Tenant anchor pool.
    pool: PgPool,
}

impl PostgresTemporalIntentStore {
    /// Construct a pending-intent store over the tenant anchor pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> PostgresTemporalIntentStore {
        PostgresTemporalIntentStore {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
        }
    }

    /// Decode one durable queue row.
    fn decode(row: &PgRow) -> Result<SequencedTemporalAppendIntent, TemporalError> {
        let raw_sequence: i64 = row.try_get("sequence").map_err(persistence)?;
        let sequence = TemporalIntentSequence::new(raw_sequence).map_err(|error| TemporalError::internal(error.to_string()))?;
        let payload: Value = row.try_get("intent").map_err(persistence)?;
        let intent = deserialize_exact(&payload).map_err(|error| TemporalError::persistence(format!("failed to decode temporal append intent: {error}")))?;
        Ok(SequencedTemporalAppendIntent { sequence, intent })
    }

    /// Claim an ordered bounded batch, optionally restricted to one entity.
    async fn claim_matching(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        entity_id: Option<&Uri>,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = claim_statement(target, owner, entity_id.map(Uri::as_str), limit.get(), lease_until).build_sqlx(PostgresQueryBuilder);
        let rows = observe_statement(INTENT_CLAIM, sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool))
            .await
            .map_err(persistence)?;
        let mut claimed = rows
            .iter()
            .map(|row| {
                let attempts: i32 = row.try_get("attempt_count").map_err(persistence)?;
                let attempts = u32::try_from(attempts).map_err(|error| TemporalError::internal(format!("negative temporal intent attempt count: {error}")))?;
                Ok(ClaimedTemporalAppendIntent {
                    pending: PostgresTemporalIntentStore::decode(row)?,
                    attempts: attempts.saturating_sub(1),
                })
            })
            .collect::<Result<Vec<_>, TemporalError>>()?;
        claimed.sort_by_key(|claim| claim.pending.sequence);
        Ok(claimed)
    }
}

#[async_trait]
impl PendingTemporalIntentReader for PostgresTemporalIntentStore {
    async fn pending_for_entity(&self, target: &ContextTarget, entity_id: &Uri) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = pending_statement(target, Some(entity_id.as_str())).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence)?;
        rows.iter().map(PostgresTemporalIntentStore::decode).collect()
    }

    async fn pending_intents(&self, target: &ContextTarget) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = pending_statement(target, None).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence)?;
        rows.iter().map(PostgresTemporalIntentStore::decode).collect()
    }
}

#[async_trait]
impl TemporalIntentQueue for PostgresTemporalIntentStore {
    async fn claim(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(target, owner, None, limit, lease_until).await
    }

    async fn claim_for_entity(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        entity_id: &Uri,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(target, owner, Some(entity_id), limit, lease_until).await
    }

    async fn acknowledge(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        intent: &SequencedTemporalAppendIntent,
    ) -> Result<LeaseAcknowledgement, TemporalError> {
        let (sql, values) =
            acknowledge_statement(target, intent.sequence.get(), intent.intent.record.write_id().as_uuid(), owner).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            INTENT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&self.pool),
        )
        .await
        .map_err(persistence)?;
        held(result.rows_affected())
    }

    async fn retry(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        intent: &SequencedTemporalAppendIntent,
        retry: TemporalIntentRetry,
    ) -> Result<LeaseAcknowledgement, TemporalError> {
        let (sql, values) = retry_statement(
            target,
            retry.eligible_at,
            &retry.summary,
            intent.sequence.get(),
            intent.intent.record.write_id().as_uuid(),
            owner,
        )
        .build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            INTENT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&self.pool),
        )
        .await
        .map_err(persistence)?;
        held(result.rows_affected())
    }
}

/// Whether a lease-guarded statement reached the one row it targeted.
///
/// Zero rows means the lease lapsed and another worker now owns the row; more
/// than one cannot happen, since the statement names the row's sequence.
fn held(rows_affected: u64) -> Result<LeaseAcknowledgement, TemporalError> {
    match rows_affected {
        0 => Ok(LeaseAcknowledgement::Lost),
        1 => Ok(LeaseAcknowledgement::Applied),
        other => Err(TemporalError::internal(format!(
            "a lease-guarded temporal intent statement reached {other} rows"
        ))),
    }
}

/// Classify one anchor-store failure at the temporal port boundary.
pub(super) fn persistence(error: impl Display) -> TemporalError {
    TemporalError::persistence(error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::intent_store::held;
    use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_statement_reaching_no_row_reports_the_lease_as_lost() {
        assert_eq!(held(0).expect("a lost lease is an outcome"), LeaseAcknowledgement::Lost);
        assert_eq!(held(1).expect("a held lease is an outcome"), LeaseAcknowledgement::Applied);
        assert!(held(2).is_err());
    }
}
