//! Yielding a running repack to a session that needs its table.
//!
//! A run holds the table's `SHARE UPDATE EXCLUSIVE` lock from the setup to
//! the swap, and its copy holds `ACCESS SHARE`. Neither conflicts with a read
//! or a write, but a schema change (a tenant migration's `ALTER TABLE` on
//! rollout) waits behind them for the whole copy, and every write of the
//! table queues behind that waiting request. So once a session the run's own
//! sessions block has waited [`YIELD_AFTER`] for a lock on the table, the run
//! cancels its own statement and is abandoned, its objects cleaned up, and
//! it is tried again after its failure back-off. A session that waits on the
//! table for someone else's lock is not yielded to: giving the table up would
//! not let it through.

use crate::{
    catalog_counter::{CatalogCounter, object_id},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::current_database,
    maintenance::{blocking_pids, cancel_backend},
    pg_type::PostgresType,
    statement::Statement,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Query, SelectStatement};
use sqlx::{PgConnection, Row};
use std::time::Duration;
use tokio::time;

/// How long a session may wait for the table before the run yields to it.
const YIELD_AFTER: Duration = Duration::from_secs(5);

/// How often the watch looks.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// One session waiting for a lock on a relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LockWaiter {
    /// The waiting backend.
    pub(crate) pid: i32,
    /// The relation it waits on.
    pub(crate) relation: u32,
    /// How long it has waited.
    pub(crate) waited: Duration,
    /// The backends holding, or queued ahead for, a lock it waits for.
    pub(crate) blockers: Vec<i32>,
}

/// What the watch makes of the waiters it saw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum YieldDecision {
    /// Another session has waited long enough for the table.
    Yield,
    /// Nobody else waits for the table long enough.
    Continue,
}

/// Whether the run on the table `relid`, whose sessions are `own`, yields to
/// one of `waiters`: another session the run blocks on the table, waiting
/// longer than [`YIELD_AFTER`].
pub(crate) fn yield_decision(waiters: &[LockWaiter], relid: u32, own: &[i32]) -> YieldDecision {
    if waiters.iter().any(|waiter| {
        waiter.relation == relid && !own.contains(&waiter.pid) && waiter.waited > YIELD_AFTER && waiter.blockers.iter().any(|blocker| own.contains(blocker))
    }) {
        YieldDecision::Yield
    } else {
        YieldDecision::Continue
    }
}

/// Watch the table `relid` through `connection` until another session has
/// waited long enough for it, then cancel the statement of the run's main
/// session `main_pid` and return. `own` are the run's sessions.
pub(crate) async fn watch(connection: &mut PgConnection, relid: u32, own: [i32; 2], main_pid: i32) -> Result<(), MaintenanceError> {
    loop {
        time::sleep(POLL_INTERVAL).await;
        let waiters = lock_waiters(connection, relid).await?;
        match yield_decision(&waiters, relid, &own) {
            YieldDecision::Continue => {}
            YieldDecision::Yield => {
                let cancel = Query::select().expr(cancel_backend(Expr::val(main_pid))).to_owned();
                cancel.to_query_scalar::<bool>().fetch_one(&mut *connection).await?;
                return Ok(());
            }
        }
    }
}

/// Every session of this database waiting for a lock on the table `relid`.
async fn lock_waiters(connection: &mut PgConnection, relid: u32) -> Result<Vec<LockWaiter>, MaintenanceError> {
    let rows = waiters_statement(relid).to_query().fetch_all(connection).await?;
    let now = Utc::now();
    rows.iter()
        .map(|row| {
            let since: DateTime<Utc> = row.try_get(2)?;
            // A wait the server's clock starts after the broker's counts as none.
            let waited = u64::try_from(now.signed_duration_since(since).num_seconds()).unwrap_or(0);
            Ok(LockWaiter {
                pid: row.try_get(0)?,
                relation: object_id(row.try_get::<i64, _>(1)?, CatalogCounter::RelationOid)?,
                waited: Duration::from_secs(waited),
                blockers: row.try_get(3)?,
            })
        })
        .collect()
}

/// The waiters on the table `relid`, each with its relation, when it began
/// to wait and the backends it waits for.
fn waiters_statement(relid: u32) -> SelectStatement {
    let current_database_oid = Query::select()
        .column(Alias::new("oid"))
        .from(Alias::new("pg_database"))
        .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
        .to_owned();
    Query::select()
        .column(Alias::new("pid"))
        .expr(cast(Expr::col(Alias::new("relation")), PostgresType::Bigint))
        .column(Alias::new("waitstart"))
        .expr(blocking_pids(Expr::col(Alias::new("pid"))))
        .from(Alias::new("pg_locks"))
        .and_where(Expr::col(Alias::new("locktype")).eq("relation"))
        .and_where(Expr::col(Alias::new("database")).in_subquery(current_database_oid))
        .and_where(Expr::col(Alias::new("relation")).eq(cast(Expr::val(i64::from(relid)), PostgresType::Oid)))
        .and_where(Expr::col(Alias::new("granted")).eq(false))
        .and_where(Expr::col(Alias::new("waitstart")).is_not_null())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::repack::yield_watch::{LockWaiter, YieldDecision, waiters_statement, yield_decision};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::time::Duration;

    const TARGET: u32 = 16_384;
    const OWN: [i32; 2] = [100, 101];

    fn waiter(pid: i32, relation: u32, seconds: u64) -> LockWaiter {
        LockWaiter {
            pid,
            relation,
            waited: Duration::from_secs(seconds),
            blockers: vec![OWN[0]],
        }
    }

    #[test]
    fn a_foreign_session_waiting_six_seconds_on_the_table_is_yielded_to() {
        assert_eq!(yield_decision(&[waiter(200, TARGET, 6)], TARGET, &OWN), YieldDecision::Yield);
    }

    #[test]
    fn a_wait_of_four_seconds_is_not_yet_yielded_to() {
        assert_eq!(yield_decision(&[waiter(200, TARGET, 4)], TARGET, &OWN), YieldDecision::Continue);
    }

    #[test]
    fn a_wait_on_another_table_is_not_yielded_to() {
        assert_eq!(yield_decision(&[waiter(200, 16_999, 60)], TARGET, &OWN), YieldDecision::Continue);
    }

    #[test]
    fn the_runs_own_guard_waiting_during_setup_is_not_yielded_to() {
        assert_eq!(yield_decision(&[waiter(101, TARGET, 30)], TARGET, &OWN), YieldDecision::Continue);
    }

    #[test]
    fn a_session_waiting_on_the_table_for_someone_elses_lock_is_not_yielded_to() {
        let blocked_elsewhere = LockWaiter {
            blockers: vec![300],
            ..waiter(200, TARGET, 60)
        };
        assert_eq!(yield_decision(&[blocked_elsewhere], TARGET, &OWN), YieldDecision::Continue);
    }

    #[test]
    fn a_session_the_guard_blocks_is_yielded_to() {
        let blocked_by_guard = LockWaiter {
            blockers: vec![300, OWN[1]],
            ..waiter(200, TARGET, 6)
        };
        assert_eq!(yield_decision(&[blocked_by_guard], TARGET, &OWN), YieldDecision::Yield);
    }

    #[test]
    fn the_waiters_are_read_from_this_databases_relation_locks() {
        let sql = waiters_statement(TARGET).to_string(PostgresQueryBuilder);
        for fragment in [
            r#""locktype" = 'relation'"#,
            r#""database" IN (SELECT "oid" FROM "pg_database" WHERE "datname" = current_database())"#,
            r#""relation" = (16384::oid)"#,
            r#""granted" = FALSE"#,
            r#""waitstart" IS NOT NULL"#,
            r#"pg_blocking_pids("pid")"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }
}
