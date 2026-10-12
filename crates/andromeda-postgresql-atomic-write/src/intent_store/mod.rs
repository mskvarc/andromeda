//! PostgreSQL anchor persistence and delivery state for temporal append intents.
//!
//! The store's enqueue, run inside the entity anchor transaction, lives in
//! [`enqueue`]; the claim, pending-read and lease-guarded statements in
//! [`lease_statements`]; the port implementations here.
//!
//! The queue lives only in the live branch: a Snapshot branch records its
//! history in the write's own transaction, so nothing here names a branch.

/// Enqueueing temporal append intents inside the entity anchor transaction.
mod enqueue;
/// The claim, pending-read and lease-guarded acknowledgement statements.
mod lease_statements;

use crate::intent_store::lease_statements::{acknowledge_statement, claim_statement, pending_statement, retry_statement};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_context_store::json_text::JsonText;
use andromeda_postgresql_pool::persistence_classification::classify;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use andromeda_temporal::{
    append_intent::{KeyedTemporalAppendIntent, TemporalIntentKey},
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
use sqlx::{PgPool, Row, postgres::PgRow};

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

    /// Decode one durable queue row, the intent straight from the column's
    /// JSON text.
    fn decode(row: &PgRow) -> Result<KeyedTemporalAppendIntent, TemporalError> {
        let raw_key: i64 = row.try_get("intent_key").map_err(|error| persistence(&error))?;
        let key = TemporalIntentKey::new(raw_key).map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error)))?;
        let payload: JsonText<'_> = row.try_get("payload").map_err(|error| persistence(&error))?;
        let intent = serde_json::from_slice(payload.as_bytes()).map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error)))?;
        Ok(KeyedTemporalAppendIntent { key, intent })
    }

    /// Claim an ordered bounded batch, optionally restricted to one entity.
    async fn claim_matching(
        &self,
        owner: LeaseHolder,
        entity_id: Option<&Identifier>,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = claim_statement(owner, entity_id.map(Identifier::as_str), limit.get(), lease_until).build_sqlx(PostgresQueryBuilder);
        let rows = observe_statement(INTENT_CLAIM, sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool))
            .await
            .map_err(|error| persistence(&error))?;
        let mut claimed = rows
            .iter()
            .map(|row| {
                let attempts: i32 = row.try_get("attempt_count").map_err(|error| persistence(&error))?;
                let attempts = u32::try_from(attempts).map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error)))?;
                Ok(ClaimedTemporalAppendIntent {
                    pending: PostgresTemporalIntentStore::decode(row)?,
                    attempts: attempts.saturating_sub(1),
                })
            })
            .collect::<Result<Vec<_>, TemporalError>>()?;
        claimed.sort_by_key(|claim| claim.pending.key);
        Ok(claimed)
    }
}

#[async_trait]
impl PendingTemporalIntentReader for PostgresTemporalIntentStore {
    async fn pending_for_entity(&self, entity_id: &Identifier) -> Result<Vec<KeyedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = pending_statement(Some(entity_id.as_str())).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| persistence(&error))?;
        rows.iter().map(PostgresTemporalIntentStore::decode).collect()
    }

    async fn pending_intents(&self) -> Result<Vec<KeyedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = pending_statement(None).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| persistence(&error))?;
        rows.iter().map(PostgresTemporalIntentStore::decode).collect()
    }
}

#[async_trait]
impl TemporalIntentQueue for PostgresTemporalIntentStore {
    async fn claim(
        &self,
        owner: LeaseHolder,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(owner, None, limit, lease_until).await
    }

    async fn claim_for_entity(
        &self,
        owner: LeaseHolder,
        entity_id: &Identifier,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(owner, Some(entity_id), limit, lease_until).await
    }

    async fn acknowledge(&self, owner: LeaseHolder, intent: &KeyedTemporalAppendIntent) -> Result<LeaseAcknowledgement, TemporalError> {
        let (sql, values) = acknowledge_statement(intent.key.get(), intent.intent.record.write_id().as_uuid(), owner).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            INTENT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&self.pool),
        )
        .await
        .map_err(|error| persistence(&error))?;
        held(result.rows_affected())
    }

    async fn retry(&self, owner: LeaseHolder, intent: &KeyedTemporalAppendIntent, retry: TemporalIntentRetry) -> Result<LeaseAcknowledgement, TemporalError> {
        let (sql, values) = retry_statement(
            retry.eligible_at,
            &retry.summary,
            intent.key.get(),
            intent.intent.record.write_id().as_uuid(),
            owner,
        )
        .build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            INTENT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&self.pool),
        )
        .await
        .map_err(|error| persistence(&error))?;
        held(result.rows_affected())
    }
}

/// Whether a lease-guarded statement reached the one row it targeted.
///
/// Zero rows means the lease lapsed and another worker now owns the row; more
/// than one cannot happen, since the statement names the row's key.
fn held(rows_affected: u64) -> Result<LeaseAcknowledgement, TemporalError> {
    match rows_affected {
        0 => Ok(LeaseAcknowledgement::Lost),
        1 => Ok(LeaseAcknowledgement::Applied),
        _more => Err(TemporalError::from(PersistenceFailure::ContractBroken.record_bare())),
    }
}

/// Classify one anchor-store failure at the temporal port boundary.
#[track_caller]
pub(super) fn persistence(error: &sqlx::Error) -> TemporalError {
    TemporalError::from(classify(error).record(error))
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
