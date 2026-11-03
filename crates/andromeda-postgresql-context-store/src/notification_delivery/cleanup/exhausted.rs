//! Reaping deliveries the worker dead-lettered.

use crate::{
    error::ContextStoreError,
    notification_delivery::cleanup::{
        branch_sweep::sweep_branches,
        retention_batch::{EventReaping, RetentionClock, purge_branch},
    },
};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;

/// Remove deliveries exhausted before `cutoff` in every branch.
///
/// A delivery is exhausted by one path only: the worker that made its last
/// attempt stamped `exhausted_at`. That instant is the retention clock, so a
/// batch is a range scan over `idx_notification_deliveries_exhausted_at`. The
/// events these deliveries leave are reaped by the orphan event pass. Returns
/// the total deliveries removed.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails for a reason
/// other than a reaped branch.
pub async fn purge_exhausted_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    sweep_branches(pool, async |target| {
        purge_branch(pool, target, RetentionClock::Exhausted, cutoff, EventReaping::Keep).await
    })
    .await
}
