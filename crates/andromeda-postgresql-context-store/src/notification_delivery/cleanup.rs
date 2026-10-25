//! Reaping delivered rows, dead-lettered rows, and the events they leave behind.
//!
//! Three independent passes cover the ways a `notification_events` row can
//! stop being owed a delivery: [`purge_delivered_before`] reaps deliveries
//! acknowledged as sent, [`purge_exhausted_before`] reaps deliveries the worker
//! gave up on and that were kept only as evidence, and
//! [`purge_orphan_events_before`] reaps events left with zero referencing
//! deliveries by any path the first two do not observe directly -- a
//! subscription that vanished between the match and the delivery insert, or a
//! `Subscription` deletion whose `ON DELETE CASCADE` removed every delivery a
//! moment the other two passes were not looking.
//!
//! Each pass sweeps every branch: the live branch and every Snapshot branch keep
//! their own queue. A branch reaped between the enumeration and the sweep shows
//! up as a missing schema and is skipped.

use crate::{
    error::ContextStoreError,
    notification_delivery::branches::{drainable_targets, is_reaped_branch},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationEvents};
use andromeda_postgresql_seaquery::{pg_array::any, statement::Statement};
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, Query};
use sqlx::postgres::PgPool;

/// Remove deliveries completed before `cutoff` in every branch, and any event
/// each sweep leaves unreferenced. Returns the total deliveries removed.
///
/// The two deletions per branch are separate statements inside one transaction,
/// and they have to be: a statement sees the table as it was when it began, so
/// an event tested for orphanhood in the same statement that removes its last
/// delivery would always still appear to have one.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement or the transaction
/// fails for a reason other than a reaped branch.
pub async fn purge_delivered_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    let mut removed = 0;
    for target in drainable_targets(pool).await? {
        match purge_delivered_in_branch(pool, &target, cutoff).await {
            Ok(count) => removed += count,
            Err(error) if is_reaped_branch(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(removed)
}

/// Remove deliveries completed before `cutoff` in one branch, reaping the events
/// their removal orphaned.
async fn purge_delivered_in_branch(pool: &PgPool, target: &ContextTarget, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    let mut tx = pool.begin().await?;
    let events: Vec<i64> = purge_delivered_statement(target, cutoff).to_query_scalar().fetch_all(&mut *tx).await?;
    let removed = events.len() as u64;

    if !events.is_empty() {
        purge_orphan_events_among_statement(target, &events).to_query().execute(&mut *tx).await?;
    }

    tx.commit().await?;
    Ok(removed)
}

/// Remove deliveries exhausted before `cutoff` in every branch.
///
/// A delivery is exhausted by one path only: the worker that made its last
/// attempt stamped `exhausted_at`. That instant is the retention clock, so the
/// reap is a range scan over `idx_notification_deliveries_exhausted_at`. Returns
/// the total deliveries removed.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails for a reason
/// other than a reaped branch.
pub async fn purge_exhausted_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    let mut removed = 0;
    for target in drainable_targets(pool).await? {
        match purge_exhausted_statement(&target, cutoff).to_query().execute(pool).await {
            Ok(result) => removed += result.rows_affected(),
            Err(error) => {
                let error = ContextStoreError::from(error);
                if is_reaped_branch(&error) {
                    continue;
                }
                return Err(error);
            }
        }
    }
    Ok(removed)
}

/// Remove events created before `cutoff` that no delivery references, in every
/// branch. Returns the total events removed.
///
/// This is the standalone reap the insert side documents: a write that matched
/// a `Subscription` which disappeared before the delivery insert ran leaves an
/// event with zero deliveries, and a cascade-deleted `Subscription` leaves its
/// event's last delivery removed outside of any cleanup pass. Neither case is
/// reached by [`purge_delivered_before`] or [`purge_exhausted_before`], so this
/// pass scans for orphanhood directly.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails for a reason
/// other than a reaped branch.
pub async fn purge_orphan_events_before(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, ContextStoreError> {
    let mut removed = 0;
    for target in drainable_targets(pool).await? {
        match purge_orphan_events_before_statement(&target, cutoff).to_query().execute(pool).await {
            Ok(result) => removed += result.rows_affected(),
            Err(error) => {
                let error = ContextStoreError::from(error);
                if is_reaped_branch(&error) {
                    continue;
                }
                return Err(error);
            }
        }
    }
    Ok(removed)
}

/// Delete deliveries acknowledged before `cutoff` in `target`'s branch,
/// returning the events they referenced so orphanhood can be re-tested against
/// the post-deletion table.
fn purge_delivered_statement(target: &ContextTarget, cutoff: DateTime<Utc>) -> DeleteStatement {
    Query::delete()
        .from_table(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_not_null())
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).lt(cutoff))
        .returning_col(NotificationDeliveries::EventKey)
        .to_owned()
}

/// Delete the events among `event_keys` that no delivery references any longer,
/// in `target`'s branch.
fn purge_orphan_events_among_statement(target: &ContextTarget, event_keys: &[i64]) -> DeleteStatement {
    let referencing_delivery = Query::select()
        .expr(Expr::val(1))
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::EventKey)).equals((NotificationEvents::Table, NotificationEvents::EventKey)),
        )
        .to_owned();
    Query::delete()
        .from_table(table_of(target, NotificationEvents::Table))
        .and_where(Expr::col(NotificationEvents::EventKey).eq(any(Expr::val(event_keys.to_vec()))))
        .and_where(Expr::exists(referencing_delivery).not())
        .to_owned()
}

/// Delete deliveries stamped exhausted before `cutoff` in `target`'s branch.
fn purge_exhausted_statement(target: &ContextTarget, cutoff: DateTime<Utc>) -> DeleteStatement {
    Query::delete()
        .from_table(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).is_not_null())
        .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).lt(cutoff))
        .to_owned()
}

/// Delete events predating `cutoff` that no delivery references, in `target`'s branch.
fn purge_orphan_events_before_statement(target: &ContextTarget, cutoff: DateTime<Utc>) -> DeleteStatement {
    let referencing_delivery = Query::select()
        .expr(Expr::val(1))
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::EventKey)).equals((NotificationEvents::Table, NotificationEvents::EventKey)),
        )
        .to_owned();
    Query::delete()
        .from_table(table_of(target, NotificationEvents::Table))
        .and_where(Expr::col(NotificationEvents::CreatedAt).lt(cutoff))
        .and_where(Expr::exists(referencing_delivery).not())
        .to_owned()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::notification_delivery::cleanup::{
        purge_delivered_statement,
        purge_exhausted_statement,
        purge_orphan_events_among_statement,
        purge_orphan_events_before_statement,
    };
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        ContextTarget::Snapshot(SnapshotTarget::published(key, id))
    }

    #[test]
    fn the_delivered_reap_returns_the_events_it_freed() {
        let sql = purge_delivered_statement(&ContextTarget::Live, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#""delivered_at" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#"RETURNING "event_key""#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_reap_qualifies_its_relations() {
        let sql = purge_delivered_statement(&snapshot(), Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "snap_5"."notification_deliveries""#), "{sql}");
    }

    #[test]
    fn the_orphan_reap_among_freed_events_requires_no_referencing_delivery() {
        let sql = purge_orphan_events_among_statement(&ContextTarget::Live, &[1, 2]).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_events""#), "{sql}");
        assert!(sql.contains("= ANY("), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn the_exhausted_reap_removes_deliveries_stamped_exhausted_before_the_cutoff() {
        let sql = purge_exhausted_statement(&ContextTarget::Live, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" <"#), "{sql}");
        assert!(!sql.contains("attempt_count") && !sql.contains("notification_events"), "{sql}");
    }

    #[test]
    fn the_standalone_orphan_reap_scans_events_by_age() {
        let sql = purge_orphan_events_before_statement(&ContextTarget::Live, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "notification_events""#), "{sql}");
        assert!(sql.contains(r#""created_at" <"#), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }
}
