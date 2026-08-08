//! Repository for the `pending_side_effects` durable queue.
//!
//! The `SideEffectQueue` trait impl lives in the binary crate, because the
//! trait and its associated types are defined in the binary's service layer.
//! This module provides the struct, constructor, and the `enqueue_in_tx`
//! inherent method.

use crate::{error::ContextStoreError, pending_side_effects::payload::PersistedBatchSideEffectJob};
use sqlx::{Postgres, Row, Transaction, postgres::PgPool};
use uuid::Uuid;

/// Postgres-backed queue for deferred batch side-effects.
#[derive(Debug)]
pub struct PendingSideEffectsRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl PendingSideEffectsRepository {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> PendingSideEffectsRepository {
        PendingSideEffectsRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Borrow the pool for trait impls defined outside this crate.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Append a side-effect job within an existing transaction. The row
    /// becomes visible to workers only when the enclosing tx commits, so a
    /// rolled-back entity write guarantees no orphaned queue entry.
    pub async fn enqueue_in_tx(tx: &mut Transaction<'_, Postgres>, payload: &PersistedBatchSideEffectJob) -> Result<Uuid, ContextStoreError> {
        let payload_json =
            serde_json::to_value(payload).map_err(|err| ContextStoreError::Internal(format!("failed to encode pending_side_effects payload: {err}")))?;
        let row = sqlx::query("INSERT INTO pending_side_effects (payload) VALUES ($1) RETURNING id")
            .bind(payload_json)
            .fetch_one(&mut **tx)
            .await?;
        let id: Uuid = row.get("id");
        Ok(id)
    }
}
