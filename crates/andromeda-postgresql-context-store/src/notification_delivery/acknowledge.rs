//! Releasing a claimed delivery back to the queue, or out of it.
//!
//! Every statement here is guarded by the claim it was taken under: it names
//! the worker that holds the lease and requires that lease to still be live.
//! A worker whose lease expired while it was delivering therefore updates
//! nothing instead of overwriting the progress of the worker that reclaimed the
//! row, and the caller learns that from the returned [`LeaseAcknowledgement`]
//! rather than from a silent no-op.
//!
//! The failure diagnostic arrives already trimmed to the length the schema
//! accepts, because the bound is a delivery-policy decision the notification
//! crate owns. Nothing here shortens it a second time: what is stored is
//! exactly what the worker recorded.

use crate::error::ContextStoreError;
use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;

/// The attempt count an exhausted delivery carries.
///
/// Exhaustion is expressed by pushing the counter past any reachable ceiling
/// rather than by a separate flag, so the one claim predicate that already
/// excludes deliveries over the configured maximum excludes dead ones too.
///
/// Shared with [`crate::notification_delivery::cleanup`], which selects
/// exhausted rows by this same sentinel when applying dead-letter retention.
pub(crate) const EXHAUSTED_ATTEMPT_COUNT: i32 = i32::MAX;

/// Record that `worker_id` delivered the notification.
///
/// The claim triple stays in place: `delivered_at` already makes the delivery
/// unclaimable, and the recorded owner is the only trace of which worker sent
/// it. Clearing the lease alone would leave two of the three columns populated,
/// which the schema's claim coherence constraint rejects.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_delivered(pool: &PgPool, id: i64, worker_id: &str) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let result = sqlx::query(
        r#"
        UPDATE notification_deliveries
        SET delivered_at = NOW()
        WHERE id = $1 AND claimed_by = $2 AND lease_until > NOW()
        "#,
    )
    .bind(id)
    .bind(worker_id)
    .execute(pool)
    .await?;
    Ok(acknowledgement(result.rows_affected()))
}

/// Record a failed attempt, release the lease and schedule the next attempt.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_failed(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    failure: &str,
    next_attempt_at: DateTime<Utc>,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let result = sqlx::query(
        r#"
        UPDATE notification_deliveries
        SET attempt_count = attempt_count + 1,
            delivery_error = $3,
            next_attempt_at = $4,
            claimed_at = NULL,
            claimed_by = NULL,
            lease_until = NULL
        WHERE id = $1 AND claimed_by = $2 AND lease_until > NOW()
        "#,
    )
    .bind(id)
    .bind(worker_id)
    .bind(failure)
    .bind(next_attempt_at)
    .execute(pool)
    .await?;
    Ok(acknowledgement(result.rows_affected()))
}

/// Record a terminal failure and make the delivery permanently unclaimable.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_exhausted(pool: &PgPool, id: i64, worker_id: &str, failure: &str) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let result = sqlx::query(
        r#"
        UPDATE notification_deliveries
        SET attempt_count = $4,
            delivery_error = $3,
            claimed_at = NULL,
            claimed_by = NULL,
            lease_until = NULL
        WHERE id = $1 AND claimed_by = $2 AND lease_until > NOW()
        "#,
    )
    .bind(id)
    .bind(worker_id)
    .bind(failure)
    .bind(EXHAUSTED_ATTEMPT_COUNT)
    .execute(pool)
    .await?;
    Ok(acknowledgement(result.rows_affected()))
}

/// Decide whether a lease-guarded update reached the delivery this worker owned.
///
/// The guarded predicate matches at most one row, so a zero count means the
/// worker's lease had expired and another worker holds the delivery.
const fn acknowledgement(rows_affected: u64) -> LeaseAcknowledgement {
    if rows_affected == 0 {
        LeaseAcknowledgement::Lost
    } else {
        LeaseAcknowledgement::Applied
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::acknowledge::acknowledgement;
    use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;

    #[test]
    fn an_update_that_reached_no_row_reports_the_lease_as_lost() {
        pretty_assertions::assert_eq!(acknowledgement(0), LeaseAcknowledgement::Lost);
    }

    #[test]
    fn an_update_that_reached_its_row_reports_the_lease_as_held() {
        pretty_assertions::assert_eq!(acknowledgement(1), LeaseAcknowledgement::Applied);
    }
}
