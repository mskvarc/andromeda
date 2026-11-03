//! Reaping deliveries acknowledged as sent.

use crate::{
    error::ContextStoreError,
    notification_delivery::cleanup::{
        branch_sweep::sweep_branches,
        retention_batch::{EventReaping, RetentionClock, purge_branch},
    },
};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;

/// Remove deliveries completed before `cutoff` in every branch, and any event
/// each batch leaves unreferenced. Returns the total deliveries removed.
///
/// A batch deletes its deliveries and then the events among them that no
/// delivery references, two statements in one transaction: a statement sees the
/// table as it was when it began, so an event tested for orphanhood in the
/// statement that removes its last delivery would always still appear to have
/// one. The events a batch holds are its own, never the pass's.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement or the transaction
/// fails for a reason other than a reaped branch.
pub async fn purge_delivered_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    sweep_branches(pool, async |target| {
        purge_branch(pool, target, RetentionClock::Delivered, cutoff, EventReaping::Reap).await
    })
    .await
}
