//! Leasing due deliveries and reading the events they answer.

use crate::{
    error::ContextStoreError,
    notification_delivery::rows::{ClaimedDelivery, ClaimedDeliveryError, RawClaimedDeliveryRow},
};
use andromeda_notification::delivery_lease::LeaseTerm;
use sqlx::postgres::PgPool;

/// Claim up to `batch_size` due deliveries for `worker_id` under `lease`.
///
/// A row is claimable only while no live lease covers it, so the claim excludes
/// deliveries another worker still owns rather than handing out a second
/// delivery of the same notification the moment the first claim commits.
///
/// The scan and the update touch `notification_deliveries` alone. The events
/// are joined onto the claimed batch afterwards, so the payload rows are read
/// once per claimed delivery instead of being locked and rewritten with it.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails. A single
/// unreadable row is reported inside the returned vector rather than as a
/// failure of the claim, because one row nobody can decode must not stall every
/// other pending notification.
pub async fn claim_due(
    pool: &PgPool,
    worker_id: &str,
    lease: LeaseTerm,
    batch_size: i32,
    max_attempts: i32,
) -> Result<Vec<Result<ClaimedDelivery, ClaimedDeliveryError>>, ContextStoreError> {
    let rows = sqlx::query_as::<_, RawClaimedDeliveryRow>(CLAIM_DUE_SQL)
        .bind(worker_id)
        .bind(batch_size)
        .bind(max_attempts)
        .bind(f64::from(lease.seconds()))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(ClaimedDelivery::try_from).collect())
}

/// Statement claiming a batch of deliveries and reading their events.
///
/// The candidate predicate is exactly the one the queue's partial claim index
/// carries: undelivered, below the attempt ceiling, due, and either unleased or
/// held under a lease that has expired. A predicate that drifted from the index
/// would leave the index unusable rather than incorrect, which is the failure
/// mode that shows up only under load.
///
/// The changed attributes are resolved back through the dictionary inside the
/// statement, preserving the order the event recorded them in: the event stores
/// four-byte keys, and a worker builds a notification from expanded IRIs.
const CLAIM_DUE_SQL: &str = r#"
WITH claimable AS (
    SELECT id
    FROM notification_deliveries
    WHERE delivered_at IS NULL
      AND attempt_count < $3
      AND next_attempt_at <= NOW()
      AND (lease_until IS NULL OR lease_until <= NOW())
    ORDER BY next_attempt_at
    FOR UPDATE SKIP LOCKED
    LIMIT $2
), claimed AS (
    UPDATE notification_deliveries d
    SET claimed_at = NOW(),
        claimed_by = $1,
        lease_until = NOW() + make_interval(secs => $4)
    FROM claimable c
    WHERE d.id = c.id
    RETURNING d.id, d.event_id, d.subscription_key, d.attempt_count, d.next_attempt_at
)
SELECT c.id,
       s.id AS subscription_id,
       c.attempt_count,
       c.next_attempt_at,
       e.id AS event_id,
       e.write_id,
       e.event_ordinal,
       e.trigger,
       e.entity_id,
       COALESCE(
           (SELECT array_agg(n.name ORDER BY k.position)
            FROM UNNEST(e.changed_attribute_name_ids) WITH ORDINALITY AS k(name_id, position)
            JOIN attribute_names n ON n.id = k.name_id),
           '{}'::text[]
       ) AS changed_attributes,
       e.entity_snapshot,
       e.previous_snapshot,
       e.created_at
FROM claimed c
JOIN notification_events e ON e.id = c.event_id
JOIN subscriptions s ON s.subscription_key = c.subscription_key
"#;

#[cfg(test)]
mod tests {
    use crate::notification_delivery::claim::CLAIM_DUE_SQL;

    #[test]
    fn the_claim_scans_and_updates_deliveries_alone() {
        let before_join = CLAIM_DUE_SQL.split("FROM claimed c").next().expect("the claiming branches");
        assert!(
            !before_join.contains("notification_events"),
            "the events table must not be locked or scanned by the claim itself"
        );
        assert!(before_join.contains("UPDATE notification_deliveries d"));
    }

    #[test]
    fn the_candidate_predicate_matches_the_partial_claim_index() {
        assert!(CLAIM_DUE_SQL.contains("WHERE delivered_at IS NULL"));
        assert!(CLAIM_DUE_SQL.contains("AND attempt_count < $3"));
        assert!(CLAIM_DUE_SQL.contains("AND next_attempt_at <= NOW()"));
        assert!(CLAIM_DUE_SQL.contains("AND (lease_until IS NULL OR lease_until <= NOW())"));
    }

    #[test]
    fn a_row_another_worker_holds_is_skipped_rather_than_waited_on() {
        assert!(CLAIM_DUE_SQL.contains("FOR UPDATE SKIP LOCKED"));
    }

    #[test]
    fn the_claim_takes_a_lease_alongside_the_claim_triple() {
        assert!(CLAIM_DUE_SQL.contains("claimed_at = NOW()"));
        assert!(CLAIM_DUE_SQL.contains("claimed_by = $1"));
        assert!(CLAIM_DUE_SQL.contains("lease_until = NOW() + make_interval(secs => $4)"));
    }

    #[test]
    fn the_changed_attributes_keep_the_order_the_event_recorded_them_in() {
        assert!(CLAIM_DUE_SQL.contains("array_agg(n.name ORDER BY k.position)"));
        assert!(CLAIM_DUE_SQL.contains("WITH ORDINALITY AS k(name_id, position)"));
    }
}
