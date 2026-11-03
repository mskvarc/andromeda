//! Reaping events no delivery references.
//!
//! `notification_events` has no ordered index on `created_at`, only a block
//! range summary, so the standalone pass walks `event_key` in ranges of
//! [`PURGE_BATCH_ROWS`] keys: in each range it deletes the events created before
//! the cutoff that no delivery references. One walk covers every candidate
//! once, and a statement touches one range whatever the table holds.

use crate::{
    error::ContextStoreError,
    notification_delivery::cleanup::branch_sweep::{PURGE_BATCH_ROWS, sweep_branches},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationEvents};
use andromeda_postgresql_seaquery::{pg_array::is_any_of, statement::Statement};
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, Func, Iden, Order, Query, SelectStatement};
use sqlx::postgres::PgPool;

/// The alias of the range of event keys one step of the walk reads.
#[derive(Iden, Clone, Copy)]
enum KeyRange {
    /// Alias: `key_range`
    Table,
}

/// Remove events created before `cutoff` that no delivery references, in every
/// branch. Returns the total events removed.
///
/// This is the standalone reap the insert side documents: a write that matched
/// a `Subscription` which disappeared before the delivery insert ran leaves an
/// event with zero deliveries, and a cascade-deleted `Subscription` leaves its
/// event's last delivery removed outside of any cleanup pass. Neither case is
/// reached by the delivered or exhausted pass, so this pass scans for orphanhood
/// directly.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails for a reason
/// other than a reaped branch.
pub async fn purge_orphan_events_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    sweep_branches(pool, async |target| purge_orphans_in_branch(pool, target, cutoff).await).await
}

/// Walk `target`'s event keys range by range, removing the orphans among them.
async fn purge_orphans_in_branch(pool: &PgPool, target: &ContextTarget, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    let mut removed = 0;
    let mut after = 0_i64;
    loop {
        let (upper, keys): (Option<i64>, i64) = key_range_statement(target, after).to_query_as().fetch_one(pool).await?;
        let Some(upper) = upper else {
            return Ok(removed);
        };
        removed += purge_orphans_in_range_statement(target, after, upper, cutoff)
            .to_query()
            .execute(pool)
            .await?
            .rows_affected();
        if keys < i64::from(PURGE_BATCH_ROWS) {
            return Ok(removed);
        }
        after = upper;
    }
}

/// The greatest key and the number of keys among the first [`PURGE_BATCH_ROWS`]
/// event keys after `after`; the greatest key is `NULL` past the last event.
fn key_range_statement(target: &ContextTarget, after: i64) -> SelectStatement {
    let keys = Query::select()
        .column(NotificationEvents::EventKey)
        .from(table_of(target, NotificationEvents::Table))
        .and_where(Expr::col(NotificationEvents::EventKey).gt(after))
        .order_by(NotificationEvents::EventKey, Order::Asc)
        .limit(u64::from(PURGE_BATCH_ROWS))
        .take();
    Query::select()
        .expr(Func::max(Expr::col(NotificationEvents::EventKey)))
        .expr(Func::count(Expr::col(NotificationEvents::EventKey)))
        .from_subquery(keys, KeyRange::Table)
        .take()
}

/// Delete the events among keys `after` (exclusive) to `upper` (inclusive)
/// created before `cutoff` that no delivery references, in `target`'s branch.
fn purge_orphans_in_range_statement(target: &ContextTarget, after: i64, upper: i64, cutoff: DateTime<Utc>) -> DeleteStatement {
    Query::delete()
        .from_table(table_of(target, NotificationEvents::Table))
        .and_where(Expr::col(NotificationEvents::EventKey).gt(after))
        .and_where(Expr::col(NotificationEvents::EventKey).lte(upper))
        .and_where(Expr::col(NotificationEvents::CreatedAt).lt(cutoff))
        .and_where(Expr::exists(referencing_delivery(target)).not())
        .to_owned()
}

/// Delete the events among `event_keys` that no delivery references any longer,
/// in `target`'s branch.
pub(crate) fn orphan_events_among_statement(target: &ContextTarget, event_keys: &[i64]) -> DeleteStatement {
    Query::delete()
        .from_table(table_of(target, NotificationEvents::Table))
        .and_where(is_any_of(Expr::col(NotificationEvents::EventKey), event_keys.iter().copied()))
        .and_where(Expr::exists(referencing_delivery(target)).not())
        .to_owned()
}

/// The deliveries that reference the event of the enclosing statement.
fn referencing_delivery(target: &ContextTarget) -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::EventKey)).equals((NotificationEvents::Table, NotificationEvents::EventKey)),
        )
        .take()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::cleanup::orphan_events::{key_range_statement, orphan_events_among_statement, purge_orphans_in_range_statement};
    use andromeda_context_space::context_target::ContextTarget;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_orphan_reap_among_freed_events_requires_no_referencing_delivery() {
        let sql = orphan_events_among_statement(&ContextTarget::Live, &[1, 2]).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_events""#), "{sql}");
        assert!(sql.contains("= ANY("), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn the_standalone_orphan_reap_deletes_inside_one_key_range() {
        let sql = purge_orphans_in_range_statement(&ContextTarget::Live, 10_000, 20_000, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_events""#), "{sql}");
        assert!(sql.contains(r#""event_key" > 10000"#), "{sql}");
        assert!(sql.contains(r#""event_key" <= 20000"#), "{sql}");
        assert!(sql.contains(r#""created_at" <"#), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn a_key_range_reads_at_most_one_batch_of_keys_in_key_order() {
        let sql = key_range_statement(&ContextTarget::Live, 30_000).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"MAX("event_key")"#), "{sql}");
        assert!(sql.contains(r#"COUNT("event_key")"#), "{sql}");
        assert!(sql.contains(r#""event_key" > 30000"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "event_key" ASC LIMIT 10000"#), "{sql}");
    }
}
