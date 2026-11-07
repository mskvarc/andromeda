//! One batch of deliveries past a retention clock, and the walk over a branch.
//!
//! A batch selects its rows in clock order with `FOR UPDATE SKIP LOCKED`, so a
//! row a worker holds is left for the next pass, and deletes them in the same
//! statement. The next batch starts after the last `(clock, delivery_key)` of
//! the one before, so it does not scan the index entries of rows deleted before
//! vacuum has run. A batch that removes fewer rows than it may ends the walk.

use crate::{
    error::ContextStoreError,
    notification_delivery::cleanup::{branch_sweep::PURGE_BATCH_ROWS, orphan_events::orphan_events_among_statement},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::NotificationDeliveries;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, LockBehavior, LockType, Order, Query};
use sqlx::postgres::PgPool;

/// The instant a delivery's retention is counted from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetentionClock {
    /// When the delivery was acknowledged as sent.
    Delivered,
    /// When the worker gave the delivery up.
    Exhausted,
}

impl RetentionClock {
    /// The column that holds the instant, which a partial index covers.
    const fn column(self) -> NotificationDeliveries {
        match self {
            RetentionClock::Delivered => NotificationDeliveries::DeliveredAt,
            RetentionClock::Exhausted => NotificationDeliveries::ExhaustedAt,
        }
    }
}

/// Whether a batch also removes the events its deliveries leave unreferenced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EventReaping {
    /// Remove them in the transaction of the batch.
    Reap,
    /// Leave them to the orphan event pass.
    Keep,
}

/// The last row a batch removed, which the next batch starts after.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RetentionCursor {
    /// The retention clock of the row.
    at: DateTime<Utc>,
    /// The key of the row, which orders rows of one instant.
    key: i64,
}

/// A removed delivery as the batch returns it: the event it answered, its
/// clock and its key.
type RemovedDelivery = (i64, DateTime<Utc>, i64);

/// Remove the deliveries of `target`'s branch past `cutoff` on `clock`, one
/// batch and one transaction at a time. Returns the deliveries removed.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a batch fails.
pub(crate) async fn purge_branch(
    pool: &PgPool,
    target: &ContextTarget,
    clock: RetentionClock,
    cutoff: DateTime<Utc>,
    events: EventReaping,
) -> Result<u64, ContextStoreError> {
    let mut removed = 0;
    let mut after = None;
    loop {
        let (count, last) = purge_batch(pool, target, clock, cutoff, after, events).await?;
        removed += count;
        match last {
            Some(cursor) if count >= u64::from(PURGE_BATCH_ROWS) => after = Some(cursor),
            Some(_) | None => return Ok(removed),
        }
    }
}

/// Remove one batch, reaping its orphaned events when `events` asks, and
/// report how many rows it removed and the last of them.
async fn purge_batch(
    pool: &PgPool,
    target: &ContextTarget,
    clock: RetentionClock,
    cutoff: DateTime<Utc>,
    after: Option<RetentionCursor>,
    events: EventReaping,
) -> Result<(u64, Option<RetentionCursor>), ContextStoreError> {
    let mut tx = pool.begin().await?;
    let removed: Vec<RemovedDelivery> = batch_statement(target, clock, cutoff, after).to_query_as().fetch_all(&mut *tx).await?;
    if events == EventReaping::Reap && !removed.is_empty() {
        let event_keys: Vec<i64> = removed.iter().map(|(event_key, ..)| *event_key).collect();
        orphan_events_among_statement(target, &event_keys).to_query().execute(&mut *tx).await?;
    }
    tx.commit().await?;
    let last = removed.iter().map(|&(_, at, key)| RetentionCursor { at, key }).max();
    Ok((removed.len() as u64, last))
}

/// Delete the first [`PURGE_BATCH_ROWS`] deliveries past `cutoff` on `clock`
/// after `after`, in clock order, returning the event, clock and key of each.
fn batch_statement(target: &ContextTarget, clock: RetentionClock, cutoff: DateTime<Utc>, after: Option<RetentionCursor>) -> DeleteStatement {
    let column = clock.column();
    let mut batch = Query::select();
    batch
        .column(NotificationDeliveries::DeliveryKey)
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(column).is_not_null())
        .and_where(Expr::col(column).lt(cutoff));
    if let Some(cursor) = after {
        batch.and_where(Expr::col(column).gte(cursor.at)).and_where(
            Expr::col(column)
                .gt(cursor.at)
                .or(Expr::col(NotificationDeliveries::DeliveryKey).gt(cursor.key)),
        );
    }
    batch
        .order_by(column, Order::Asc)
        .order_by(NotificationDeliveries::DeliveryKey, Order::Asc)
        .limit(u64::from(PURGE_BATCH_ROWS))
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    Query::delete()
        .from_table(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(NotificationDeliveries::DeliveryKey).in_subquery(batch))
        .returning(Query::returning().columns([NotificationDeliveries::EventKey, column, NotificationDeliveries::DeliveryKey]))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::cleanup::retention_batch::{RetentionClock, RetentionCursor, batch_statement};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        ContextTarget::Snapshot(SnapshotTarget::published(key, id))
    }

    fn sql(clock: RetentionClock, after: Option<RetentionCursor>) -> String {
        batch_statement(&ContextTarget::Live, clock, Utc::now(), after).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_delivered_batch_is_bounded_ordered_and_skips_locked_rows() {
        let sql = sql(RetentionClock::Delivered, None);
        assert!(sql.contains(r#"DELETE FROM "notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#""delivered_at" IS NOT NULL"#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "delivered_at" ASC, "delivery_key" ASC LIMIT 10000 FOR UPDATE SKIP LOCKED"#),
            "{sql}"
        );
        assert!(sql.contains(r#"RETURNING "event_key", "delivered_at", "delivery_key""#), "{sql}");
    }

    #[test]
    fn an_exhausted_batch_reads_the_exhausted_clock() {
        let sql = sql(RetentionClock::Exhausted, None);
        assert!(sql.contains(r#""exhausted_at" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "exhausted_at" ASC, "delivery_key" ASC LIMIT 10000"#), "{sql}");
        assert!(!sql.contains("delivered_at"), "{sql}");
    }

    #[test]
    fn a_later_batch_starts_after_the_last_row_of_the_one_before() {
        let cursor = RetentionCursor { at: Utc::now(), key: 77 };
        let sql = sql(RetentionClock::Delivered, Some(cursor));
        assert!(sql.contains(r#""delivered_at" >="#), "{sql}");
        assert!(sql.contains(r#"OR "delivery_key" > 77"#), "{sql}");
    }

    #[test]
    fn a_first_batch_has_no_cursor_condition() {
        let sql = sql(RetentionClock::Delivered, None);
        assert!(!sql.contains(r#""delivery_key" >"#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_batch_qualifies_its_relations() {
        let sql = batch_statement(&snapshot(), RetentionClock::Delivered, Utc::now(), None).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "snap_5"."notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#"FROM "snap_5"."notification_deliveries""#), "{sql}");
    }
}
