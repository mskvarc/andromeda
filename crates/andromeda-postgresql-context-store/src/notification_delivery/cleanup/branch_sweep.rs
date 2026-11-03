//! Visiting every branch a purge pass covers.

use crate::{
    error::ContextStoreError,
    notification_delivery::branches::{drainable_targets, is_reaped_branch},
};
use andromeda_context_space::context_target::ContextTarget;
use sqlx::postgres::PgPool;

/// The most rows one purge statement removes, and the most event keys a pass
/// holds in memory at once.
pub(crate) const PURGE_BATCH_ROWS: u32 = 10_000;

/// Run `purge` on every branch and add up what it removed.
///
/// A branch reaped between the enumeration and its purge shows up as a missing
/// schema and is skipped.
///
/// # Errors
///
/// Returns the failure of the enumeration, or of a purge for a reason other than
/// a reaped branch.
pub(crate) async fn sweep_branches(pool: &PgPool, purge: impl AsyncFn(&ContextTarget) -> Result<u64, ContextStoreError>) -> Result<u64, ContextStoreError> {
    let mut removed = 0;
    for target in drainable_targets(pool).await? {
        match purge(&target).await {
            Ok(count) => removed += count,
            Err(error) if is_reaped_branch(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(removed)
}
