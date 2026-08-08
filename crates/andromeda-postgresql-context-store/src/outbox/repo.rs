//! The PostgreSQL notification outbox.
//!
//! The `OutboxStore` trait impl lives in the binary crate, because the trait
//! and its associated types (`OutboxEntry`, `WorkerInstanceId`, etc.) are
//! defined in the binary's service layer. This module provides the struct
//! and the inherent methods that the binary's trait impl delegates to.

use crate::error::ContextStoreError;
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
    /// Each entry is described by its pre-rendered column values so the
    /// domain types stay in the binary's service layer.
    pub async fn insert_batch_raw_in_tx(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, entries: &[OutboxInsertRow<'_>]) -> Result<(), ContextStoreError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO notification_outbox \
             (subscription_id, entity_id, trigger, entity_snapshot, previous_snapshot, changed_attrs) ",
        );
        builder.push_values(entries, |mut b, entry| {
            b.push_bind(entry.subscription_id)
                .push_bind(entry.entity_id)
                .push_bind(entry.trigger)
                .push_bind(entry.entity_snapshot)
                .push_bind(entry.previous_snapshot)
                .push_bind(entry.changed_attrs);
        });
        builder.build().execute(&mut **tx).await?;

        Ok(())
    }
}

/// Pre-rendered column values for one outbox insert row.
///
/// The domain → string conversion happens in the binary's service layer;
/// this struct carries the already-rendered values so the context-store
/// does not need the domain types.
#[derive(Debug)]
pub struct OutboxInsertRow<'a> {
    /// Subscription identifier as a string.
    pub subscription_id: &'a str,
    /// Entity identifier as a string.
    pub entity_id: &'a str,
    /// Trigger reason as a string.
    pub trigger: &'a str,
    /// Entity snapshot JSON.
    pub entity_snapshot: &'a serde_json::Value,
    /// Previous entity snapshot JSON, if any.
    pub previous_snapshot: &'a Option<serde_json::Value>,
    /// Changed attribute names as strings.
    pub changed_attrs: &'a [String],
}
