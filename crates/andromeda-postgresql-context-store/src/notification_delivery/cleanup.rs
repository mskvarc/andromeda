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
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationEvents};
use andromeda_postgresql_seaquery::pg_array::any;
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
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
    let (sql, values) = purge_delivered_statement(cutoff).build_sqlx(PostgresQueryBuilder);
    let events: Vec<i64> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut *tx).await?;
    let removed = events.len() as u64;

    if !events.is_empty() {
        let (sql, values) = purge_orphan_events_among_statement(&events).build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut *tx).await?;
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
    let (sql, values) = purge_exhausted_statement(cutoff, max_attempts).build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool).await?;
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
    let (sql, values) = purge_orphan_events_before_statement(cutoff).build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool).await?;
    Ok(result.rows_affected())
}

/// Delete deliveries acknowledged before `cutoff`, returning the events they
/// referenced so orphanhood can be re-tested against the post-deletion table.
fn purge_delivered_statement(cutoff: DateTime<Utc>) -> DeleteStatement {
    Query::delete()
        .from_table(NotificationDeliveries::Table)
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_not_null())
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).lt(cutoff))
        .returning_col(NotificationDeliveries::EventId)
        .to_owned()
}

/// Delete the events among `event_ids` that no delivery references any longer.
///
/// The candidate set is exactly the events whose last delivery the paired
/// [`purge_delivered_statement`] just removed, so this pass never scans the
/// whole events table for the common case.
fn purge_orphan_events_among_statement(event_ids: &[i64]) -> DeleteStatement {
    let referencing_delivery = Query::select()
        .expr(Expr::val(1))
        .from(NotificationDeliveries::Table)
        .and_where(Expr::col((NotificationDeliveries::Table, NotificationDeliveries::EventId)).equals((NotificationEvents::Table, NotificationEvents::Id)))
        .to_owned();
    Query::delete()
        .from_table(NotificationEvents::Table)
        .and_where(Expr::col(NotificationEvents::Id).eq(any(Expr::val(event_ids.to_vec()))))
        .and_where(Expr::exists(referencing_delivery).not())
        .to_owned()
}

/// Delete undelivered deliveries at or past the attempt ceiling whose event
/// predates `cutoff`.
///
/// The original `DELETE ... USING notification_events` is expressed as a
/// correlated `EXISTS`: the join to the event exists only to read its
/// `created_at`, and a delivery is removed exactly when an event older than the
/// cutoff answers it, which is what the existence test asserts.
fn purge_exhausted_statement(cutoff: DateTime<Utc>, max_attempts: i32) -> DeleteStatement {
    let dead_event = Query::select()
        .expr(Expr::val(1))
        .from(NotificationEvents::Table)
        .and_where(Expr::col((NotificationEvents::Table, NotificationEvents::Id)).equals((NotificationDeliveries::Table, NotificationDeliveries::EventId)))
        .and_where(Expr::col((NotificationEvents::Table, NotificationEvents::CreatedAt)).lt(cutoff))
        .to_owned();
    Query::delete()
        .from_table(NotificationDeliveries::Table)
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::AttemptCount).gte(max_attempts))
        .and_where(Expr::exists(dead_event))
        .to_owned()
}

/// Delete events predating `cutoff` that no delivery references.
fn purge_orphan_events_before_statement(cutoff: DateTime<Utc>) -> DeleteStatement {
    let referencing_delivery = Query::select()
        .expr(Expr::val(1))
        .from(NotificationDeliveries::Table)
        .and_where(Expr::col((NotificationDeliveries::Table, NotificationDeliveries::EventId)).equals((NotificationEvents::Table, NotificationEvents::Id)))
        .to_owned();
    Query::delete()
        .from_table(NotificationEvents::Table)
        .and_where(Expr::col(NotificationEvents::CreatedAt).lt(cutoff))
        .and_where(Expr::exists(referencing_delivery).not())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::cleanup::{
        purge_delivered_statement,
        purge_exhausted_statement,
        purge_orphan_events_among_statement,
        purge_orphan_events_before_statement,
    };
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_delivered_reap_returns_the_events_it_freed() {
        let sql = purge_delivered_statement(Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#""delivered_at" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#"RETURNING "event_id""#), "{sql}");
    }

    #[test]
    fn the_orphan_reap_among_freed_events_requires_no_referencing_delivery() {
        let sql = purge_orphan_events_among_statement(&[1, 2]).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_events""#), "{sql}");
        assert!(sql.contains("= ANY("), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn the_exhausted_reap_removes_dead_deliveries_older_than_the_cutoff() {
        let sql = purge_exhausted_statement(Utc::now(), 5).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#""delivered_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""attempt_count" >= 5"#), "{sql}");
        assert!(sql.contains("EXISTS") && !sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn the_standalone_orphan_reap_scans_events_by_age() {
        let sql = purge_orphan_events_before_statement(Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_events""#), "{sql}");
        assert!(sql.contains(r#""created_at" <"#), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }
}
