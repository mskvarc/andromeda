//! Waiting out the transactions older than a repack's copy.
//!
//! A transaction already open when the copy took its snapshot may still
//! write the table, and its logged writes reach the log only once it
//! commits. The swap therefore waits until every such transaction has ended,
//! so the replay before the swap sees all of them. The copy's snapshot is the
//! first statement of its transaction, the read of every virtual transaction
//! open then, the run's own sessions and vacuum aside: a vacuum never writes
//! the table's rows, and waiting for one would hold the swap for as long as it
//! runs. Other databases' transactions count only where they hold a lock with
//! no database, which a prepared transaction can.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{introspection::current_database, maintenance::backend_pid, statement::Statement};
use sea_query::{Alias, Cond, Expr, ExprTrait, Func, JoinType, Query, SelectStatement};
use sqlx::PgConnection;
use std::time::Duration;

/// How long the swap waits for the transactions older than the copy.
const WAIT_BUDGET: Duration = Duration::from_mins(5);

/// The virtual transactions open when a copy took its snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct OldTransactions(Vec<String>);

/// Whether the swap may go on, after one look at the old transactions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OldTransactionsStep {
    /// Every old transaction has ended.
    Gone,
    /// Some are still open, within the budget.
    Waiting,
    /// Some are still open past the budget.
    TimedOut,
}

/// The step after finding `alive` old transactions still open, `waited` into
/// the wait.
pub(crate) const fn old_transactions_step(alive: u64, waited: Duration) -> OldTransactionsStep {
    if alive == 0 {
        OldTransactionsStep::Gone
    } else if waited.as_secs() >= WAIT_BUDGET.as_secs() {
        OldTransactionsStep::TimedOut
    } else {
        OldTransactionsStep::Waiting
    }
}

/// Read the virtual transactions open now, leaving out the guard session
/// `guard_pid`. Run it as the first statement of the copy's transaction, so
/// its snapshot is the copy's.
pub(crate) async fn capture(connection: &mut PgConnection, guard_pid: i32) -> Result<OldTransactions, MaintenanceError> {
    let transactions = capture_statement(guard_pid).to_query_scalar::<String>().fetch_all(connection).await?;
    Ok(OldTransactions(transactions))
}

/// How many of `old` are still open.
pub(crate) async fn still_open(connection: &mut PgConnection, old: &OldTransactions) -> Result<u64, MaintenanceError> {
    if old.0.is_empty() {
        return Ok(0);
    }
    let alive = alive_statement(old).to_query_scalar::<i64>().fetch_one(connection).await?;
    non_negative(alive, CatalogCounter::BackendPid)
}

/// The virtual transactions open now, from their own `virtualxid` locks.
fn capture_statement(guard_pid: i32) -> SelectStatement {
    let lock = |column: &'static str| Expr::col((Alias::new("l"), Alias::new(column)));
    let activity = |column: &'static str| Expr::col((Alias::new("a"), Alias::new(column)));
    let database_name = Expr::col((Alias::new("d"), Alias::new("datname")));
    Query::select()
        .expr(lock("virtualtransaction"))
        .from_as(Alias::new("pg_locks"), Alias::new("l"))
        .join_as(
            JoinType::LeftJoin,
            Alias::new("pg_stat_activity"),
            Alias::new("a"),
            Expr::col((Alias::new("a"), Alias::new("pid"))).equals((Alias::new("l"), Alias::new("pid"))),
        )
        .join_as(
            JoinType::LeftJoin,
            Alias::new("pg_database"),
            Alias::new("d"),
            Expr::col((Alias::new("d"), Alias::new("oid"))).equals((Alias::new("a"), Alias::new("datid"))),
        )
        .and_where(lock("locktype").eq("virtualxid"))
        .and_where(lock("pid").ne(backend_pid()))
        .and_where(lock("pid").ne(guard_pid))
        .and_where(lock("virtualxid").ne("1/1"))
        .and_where(lock("virtualtransaction").ne("-1/0"))
        .and_where(Func::coalesce([activity("backend_type"), Expr::val("")]).ne("autovacuum worker"))
        .and_where(Func::lower(Func::coalesce([activity("query"), Expr::val("")])).not_like("vacuum%"))
        .cond_where(
            Cond::any()
                .add(database_name.clone().is_null())
                .add(database_name.eq(current_database()))
                .add(lock("database").eq(0)),
        )
        .to_owned()
}

/// How many of `old` still hold their `virtualxid` lock.
fn alive_statement(old: &OldTransactions) -> SelectStatement {
    Query::select()
        .expr(Func::count(Expr::val(1)))
        .from(Alias::new("pg_locks"))
        .and_where(Expr::col(Alias::new("locktype")).eq("virtualxid"))
        .and_where(Expr::col(Alias::new("pid")).ne(backend_pid()))
        .and_where(Expr::col(Alias::new("virtualtransaction")).is_in(old.0.iter().map(String::as_str)))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::repack::old_transactions::{OldTransactions, OldTransactionsStep, alive_statement, capture_statement, old_transactions_step};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::time::Duration;

    #[test]
    fn the_swap_waits_for_old_transactions_for_five_minutes() {
        assert_eq!(old_transactions_step(0, Duration::from_secs(600)), OldTransactionsStep::Gone);
        assert_eq!(old_transactions_step(2, Duration::from_secs(299)), OldTransactionsStep::Waiting);
        assert_eq!(old_transactions_step(1, Duration::from_secs(300)), OldTransactionsStep::TimedOut);
    }

    #[test]
    fn the_capture_leaves_out_the_runs_own_sessions_and_vacuums() {
        let sql = capture_statement(4_242).to_string(PostgresQueryBuilder);
        for fragment in [
            r#""l"."locktype" = 'virtualxid'"#,
            r#""l"."pid" <> pg_backend_pid()"#,
            r#""l"."pid" <> 4242"#,
            r#"COALESCE("a"."backend_type", '') <> 'autovacuum worker'"#,
            r#"LOWER(COALESCE("a"."query", '')) NOT LIKE 'vacuum%'"#,
            r#""d"."datname" = current_database()"#,
            r#""l"."database" = 0"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    #[test]
    fn the_wait_counts_the_captured_transactions_still_holding_their_lock() {
        let sql = alive_statement(&OldTransactions(vec!["3/12".to_owned(), "4/7".to_owned()])).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""virtualtransaction" IN ('3/12', '4/7')"#), "{sql}");
        assert!(sql.contains(r#""pid" <> pg_backend_pid()"#), "{sql}");
    }
}
