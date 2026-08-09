//! PostgreSQL anchor persistence and delivery state for temporal append intents.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_temporal::{
    append_intent::{SequencedTemporalAppendIntent, TemporalAppendIntent, TemporalIntentSequence},
    error::TemporalError,
    port::{
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentLeaseOwner, TemporalIntentQueue, TemporalIntentRetry},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};

/// PostgreSQL-backed pending temporal intent queue for one tenant.
#[derive(Clone, Debug)]
pub struct PostgresTemporalIntentStore {
    /// Tenant anchor pool.
    pool: PgPool,
}

impl PostgresTemporalIntentStore {
    /// Construct a pending-intent store over the tenant anchor pool.
    pub fn new(pool: &PgPool) -> PostgresTemporalIntentStore {
        PostgresTemporalIntentStore {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
        }
    }

    /// Persist complete intents inside the entity anchor transaction.
    pub(crate) async fn enqueue_in_tx(tx: &mut Transaction<'_, Postgres>, intents: &[TemporalAppendIntent]) -> Result<(), TemporalError> {
        if intents.is_empty() {
            return Ok(());
        }
        let mut ids = Vec::with_capacity(intents.len());
        let mut entity_ids = Vec::with_capacity(intents.len());
        let mut payloads = Vec::with_capacity(intents.len());
        for intent in intents {
            ids.push(intent.id.as_uuid());
            entity_ids.push(intent.entity_id.as_str());
            payloads.push(serde_json::to_value(intent).map_err(|error| TemporalError::internal(format!("failed to encode temporal append intent: {error}")))?);
        }
        sqlx::query(
            r#"
            INSERT INTO temporal_append_intents (intent_id, entity_id, intent)
            SELECT input.intent_id, input.entity_id, input.intent
            FROM UNNEST($1::uuid[], $2::text[], $3::jsonb[])
                AS input(intent_id, entity_id, intent)
            "#,
        )
        .bind(&ids)
        .bind(&entity_ids)
        .bind(&payloads)
        .execute(&mut **tx)
        .await
        .map_err(persistence)?;
        Ok(())
    }

    /// Decode one durable queue row.
    fn decode(row: &sqlx::postgres::PgRow) -> Result<SequencedTemporalAppendIntent, TemporalError> {
        let raw_sequence: i64 = row.try_get("sequence").map_err(persistence)?;
        let sequence = TemporalIntentSequence::new(raw_sequence).map_err(|error| TemporalError::internal(error.to_string()))?;
        let payload: Value = row.try_get("intent").map_err(persistence)?;
        let intent =
            serde_json::from_value(payload).map_err(|error| TemporalError::persistence(format!("failed to decode temporal append intent: {error}")))?;
        Ok(SequencedTemporalAppendIntent { sequence, intent })
    }

    /// Claim an ordered bounded batch, optionally restricted to one entity.
    async fn claim_matching(
        &self,
        owner: TemporalIntentLeaseOwner,
        entity_id: Option<&Uri>,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        let rows = sqlx::query(
            r#"
            WITH eligible AS (
                SELECT sequence
                FROM temporal_append_intents
                WHERE available_at <= NOW()
                  AND (lease_until IS NULL OR lease_until <= NOW())
                  AND ($4::text IS NULL OR entity_id = $4)
                ORDER BY sequence
                FOR UPDATE SKIP LOCKED
                LIMIT $1
            )
            UPDATE temporal_append_intents AS intent
            SET lease_owner = $2, lease_until = $3, attempts = intent.attempts + 1
            FROM eligible
            WHERE intent.sequence = eligible.sequence
            RETURNING intent.sequence, intent.intent, intent.attempts
            "#,
        )
        .bind(i64::from(limit.get()))
        .bind(owner.as_uuid())
        .bind(lease_until)
        .bind(entity_id.map(Uri::as_str))
        .fetch_all(&self.pool)
        .await
        .map_err(persistence)?;
        let mut claimed = rows
            .iter()
            .map(|row| {
                let attempts: i32 = row.try_get("attempts").map_err(persistence)?;
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
    async fn pending_for_entity(&self, entity_id: &Uri) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
        let rows = sqlx::query("SELECT sequence, intent FROM temporal_append_intents WHERE entity_id = $1 ORDER BY sequence")
            .bind(entity_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(persistence)?;
        rows.iter().map(PostgresTemporalIntentStore::decode).collect()
    }

    async fn pending_intents(&self) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
        let rows = sqlx::query("SELECT sequence, intent FROM temporal_append_intents ORDER BY sequence")
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
        owner: TemporalIntentLeaseOwner,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(owner, None, limit, lease_until).await
    }

    async fn claim_for_entity(
        &self,
        owner: TemporalIntentLeaseOwner,
        entity_id: &Uri,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(owner, Some(entity_id), limit, lease_until).await
    }

    async fn acknowledge(&self, owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent) -> Result<(), TemporalError> {
        let result = sqlx::query("DELETE FROM temporal_append_intents WHERE sequence = $1 AND intent_id = $2 AND lease_owner = $3")
            .bind(intent.sequence.get())
            .bind(intent.intent.id.as_uuid())
            .bind(owner.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(persistence)?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(TemporalError::internal("temporal intent acknowledgement did not own exactly one leased row"))
        }
    }

    async fn retry(&self, owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent, retry: TemporalIntentRetry) -> Result<(), TemporalError> {
        let result = sqlx::query(
            r#"
            UPDATE temporal_append_intents
            SET available_at = $1, last_error = $2, lease_owner = NULL, lease_until = NULL
            WHERE sequence = $3 AND intent_id = $4 AND lease_owner = $5
            "#,
        )
        .bind(retry.eligible_at)
        .bind(retry.summary)
        .bind(intent.sequence.get())
        .bind(intent.intent.id.as_uuid())
        .bind(owner.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(persistence)?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(TemporalError::internal("temporal intent retry did not own exactly one leased row"))
        }
    }
}

/// Classify one anchor-store failure at the temporal port boundary.
fn persistence(error: impl std::fmt::Display) -> TemporalError {
    TemporalError::persistence(error.to_string())
}
