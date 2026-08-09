//! The PostgreSQL notification outbox.
//!
//! The `OutboxStore` trait impl lives in the binary crate, because the trait
//! and its associated types (`OutboxEntry`, `WorkerInstanceId`, etc.) are
//! defined in the binary's service layer. This module provides the struct
//! and the inherent methods that the binary's trait impl delegates to.

use crate::{
    error::ContextStoreError,
    outbox::rows::{OutboxRecord, OutboxRowError, RawOutboxRow},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use serde_json::Value;
use sqlx::{QueryBuilder, postgres::PgPool};

/// Repository for notification outbox persistence operations.
#[derive(Debug)]
pub struct OutboxRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl OutboxRepository {
    /// Create a new outbox repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> OutboxRepository {
        OutboxRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Borrow the pool for trait impls defined outside this crate.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Insert a batch of outbox entries within an existing transaction.
    ///
    /// Builds a single multi-row `INSERT ... VALUES (...), (...), ...` via
    /// [`sqlx::QueryBuilder::push_values`], collapsing N round-trips into one.
    ///
    pub async fn insert_batch_in_tx(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, entries: &[OutboxInsertRecord<'_>]) -> Result<(), ContextStoreError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO notification_outbox \
             (subscription_id, entity_id, trigger, entity_snapshot, previous_snapshot, changed_attrs) ",
        );
        builder.push_values(entries, |mut b, entry| {
            let changed_attrs = entry.changed_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>();
            b.push_bind(entry.subscription_id.as_str())
                .push_bind(entry.entity_id.as_str())
                .push_bind(entry.trigger.to_string())
                .push_bind(entry.entity_snapshot)
                .push_bind(entry.previous_snapshot)
                .push_bind(changed_attrs);
        });
        builder.build().execute(&mut **tx).await?;

        Ok(())
    }

    /// Claim due rows and validate every domain-valued column.
    pub async fn claim_pending(
        &self,
        worker_id: &str,
        batch_size: i32,
        max_attempts: i32,
    ) -> Result<Vec<Result<OutboxRecord, OutboxRowError>>, ContextStoreError> {
        let rows = sqlx::query_as::<_, RawOutboxRow>(
            r#"
            WITH claimable AS (
                SELECT id
                FROM notification_outbox
                WHERE delivered_at IS NULL
                  AND attempt_count < $3
                  AND next_attempt_at <= NOW()
                ORDER BY next_attempt_at
                FOR UPDATE SKIP LOCKED
                LIMIT $2
            )
            UPDATE notification_outbox o
            SET claimed_at = NOW(), claimed_by = $1
            FROM claimable c
            WHERE o.id = c.id
            RETURNING o.id, o.subscription_id, o.entity_id, o.trigger,
                      o.entity_snapshot, o.previous_snapshot, o.changed_attrs,
                      o.created_at, o.attempt_count, o.next_attempt_at
            "#,
        )
        .bind(worker_id)
        .bind(batch_size)
        .bind(max_attempts)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(OutboxRecord::try_from).collect())
    }
}

/// Typed values for one outbox insert row.
#[derive(Debug)]
pub struct OutboxInsertRecord<'a> {
    /// Subscription identifier.
    pub subscription_id: &'a Uri,
    /// Entity identifier.
    pub entity_id: &'a Uri,
    /// Trigger reason.
    pub trigger: NotificationTrigger,
    /// Entity snapshot JSON.
    pub entity_snapshot: &'a Value,
    /// Previous entity snapshot JSON, if any.
    pub previous_snapshot: &'a Option<Value>,
    /// Changed attribute names.
    pub changed_attrs: &'a [AttributeName],
}
