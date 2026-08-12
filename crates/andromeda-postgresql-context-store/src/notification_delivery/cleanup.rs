//! Reaping delivered rows, dead-lettered rows, and the events they leave behind.
//!
//! Three independent passes cover the ways a `notification_events` row can
//! stop being owed a delivery: [`purge_delivered_before`] reaps deliveries
//! acknowledged as sent, [`purge_exhausted_before`] reaps deliveries that ran
//! out of attempts and were kept only as evidence, and
//! [`purge_orphan_events_before`] reaps events left with zero referencing
//! deliveries by any path the first two do not observe directly -- a
//! subscription that vanished between the match and the delivery insert, or a
//! `Subscription` deletion whose `ON DELETE CASCADE` removed every delivery a
//! moment the other two passes were not looking.

use crate::error::ContextStoreError;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;

/// Remove deliveries completed before `cutoff`, and any event left unreferenced.
///
/// The two deletions are separate statements inside one transaction, and they
/// have to be: a statement sees the table as it was when it began, so an event
/// tested for orphanhood in the same statement that removes its last delivery
/// would always still appear to have one. Splitting them lets the existence
/// check see the deletions that preceded it.
///
/// Only the events whose deliveries this reap removed are considered. An event
/// keeps its payload for as long as any delivery still references it, including
/// a delivery that has exhausted its attempts and is retained as evidence of a
/// notification that never arrived.
///
/// Returns the number of deliveries removed.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement or the transaction
/// fails.
pub async fn purge_delivered_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    let mut tx = pool.begin().await?;
    let events: Vec<i64> = sqlx::query_scalar(
        r#"
        DELETE FROM notification_deliveries
        WHERE delivered_at IS NOT NULL AND delivered_at < $1
        RETURNING event_id
        "#,
    )
    .bind(cutoff)
    .fetch_all(&mut *tx)
    .await?;
    let removed = events.len() as u64;

    if !events.is_empty() {
        sqlx::query(
            r#"
            DELETE FROM notification_events e
            WHERE e.id = ANY($1)
              AND NOT EXISTS (SELECT 1 FROM notification_deliveries d WHERE d.event_id = e.id)
            "#,
        )
        .bind(&events)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(removed)
}

/// Remove exhausted deliveries whose event was created before `cutoff`.
///
/// A delivery is dead by two distinct routes: `acknowledge::mark_exhausted`
/// stamps the `i32::MAX` sentinel on a terminal failure, and an ordinary
/// retryable failure that keeps missing (`acknowledge::mark_failed`) reaches
/// the same dead end one attempt at a time, stopping at `attempt_count ==
/// max_attempts` because the claim predicate (`attempt_count < max_attempts`)
/// then excludes it forever. Matching only the sentinel reaps the first route
/// and leaves the second accumulating without limit, so this statement matches
/// the terminal condition both routes share -- `attempt_count` has reached or
/// passed the configured ceiling -- rather than the sentinel alone.
/// `notification_deliveries` carries no timestamp of its own for when
/// exhaustion happened, so the retention clock reads the immutable
/// `notification_events.created_at` of the event it answers -- the event is
/// written once, inside the same write that first queued the delivery, so its
/// age is a stable lower bound on how long the delivery has been sitting dead.
///
/// The event a dead-lettered row released is picked up by
/// [`purge_orphan_events_before`], not by this statement: the two are kept
/// separate so a delivery-owning transaction never depends on whether another
/// delivery still references the same event.
///
/// Returns the number of deliveries removed.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn purge_exhausted_before(pool: &PgPool, cutoff: DateTime<Utc>, max_attempts: i32) -> Result<u64, ContextStoreError> {
    let result = sqlx::query(
        r#"
        DELETE FROM notification_deliveries d
        USING notification_events e
        WHERE d.event_id = e.id
          AND d.delivered_at IS NULL
          AND d.attempt_count >= $1
          AND e.created_at < $2
        "#,
    )
    .bind(max_attempts)
    .bind(cutoff)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Remove events created before `cutoff` that no delivery references.
///
/// This is the standalone reap the insert side documents: a write that
/// matched a `Subscription` which disappeared before the delivery insert ran
/// leaves an event with zero deliveries from the moment it is written, and a
/// cascade-deleted `Subscription` leaves its event's last delivery removed by
/// `ON DELETE CASCADE` outside of any cleanup pass. Neither case is reached by
/// [`purge_delivered_before`] or [`purge_exhausted_before`], which only test
/// the events belonging to the rows they themselves just deleted, so this pass
/// scans for orphanhood directly instead of following a deletion.
///
/// The horizon exists so an event is not raced against the delivery insert
/// that is still in flight for it; a write matching zero subscriptions never
/// creates an event at all; and one still being inserted has not committed
/// yet, so it is invisible here regardless of a horizon.
///
/// Returns the number of events removed.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn purge_orphan_events_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    let result = sqlx::query(
        r#"
        DELETE FROM notification_events e
        WHERE e.created_at < $1
          AND NOT EXISTS (SELECT 1 FROM notification_deliveries d WHERE d.event_id = e.id)
        "#,
    )
    .bind(cutoff)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
