//! The second session that keeps schema changes off a table for a whole run.
//!
//! From the moment the setup commits until the swap, the table must keep the
//! shape the copy was made from. The guard session holds the table's
//! `SHARE UPDATE EXCLUSIVE` lock for that whole time: it conflicts with every
//! schema change, vacuum, analyze and concurrent index build, and with no
//! read or write. The guard queues its request while the setup still holds
//! `ACCESS EXCLUSIVE`, so it is next in line when the setup commits and no
//! schema change slips in between. A request that conflicts with the guard
//! and is already waiting when the setup is ready to commit is ahead of the
//! guard in the queue; the run then yields to it rather than cancel it.
//!
//! The guard idles in its transaction from the setup to the swap, so its
//! session lifts the idle-in-transaction timeout a tenant database sets. The
//! guard runs no snapshot between its statements, so it holds no vacuum
//! horizon while it idles.

use crate::error::MaintenanceError;
use andromeda_postgresql_seaquery::{
    cast::cast,
    command::Command,
    introspection::current_database,
    maintenance::backend_pid,
    pg_type::PostgresType,
    relation_lock::{LockAttempt, LockMode},
    setting::{Setting, SettingAssignment, SettingScope},
    statement::Statement,
};
use sea_query::{Alias, Expr, ExprTrait, Query, SelectStatement, TableName};
use sqlx::{PgConnection, Postgres, Row, Transaction};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};
use tokio::time;

/// The longest the guard's request waits for the setup to commit, and the
/// longest the setup waits to see the request queued.
const QUEUE_BUDGET: Duration = Duration::from_secs(60);

/// How often the setup looks for the guard's request in the lock queue.
const QUEUE_POLL: Duration = Duration::from_millis(10);

/// A lock mode `pg_locks` reports a waiting request in.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::EnumIter, strum::EnumString)]
enum WaitingMode {
    /// `AccessShare`, a read.
    #[strum(serialize = "AccessShareLock")]
    AccessShare,
    /// `RowShare`, a `SELECT ... FOR UPDATE`.
    #[strum(serialize = "RowShareLock")]
    RowShare,
    /// `RowExclusive`, a write.
    #[strum(serialize = "RowExclusiveLock")]
    RowExclusive,
    /// `ShareUpdateExclusive`: vacuum, analyze, a concurrent build.
    #[strum(serialize = "ShareUpdateExclusiveLock")]
    ShareUpdateExclusive,
    /// `Share`: a non-concurrent index build.
    #[strum(serialize = "ShareLock")]
    Share,
    /// `ShareRowExclusive`: a trigger's creation.
    #[strum(serialize = "ShareRowExclusiveLock")]
    ShareRowExclusive,
    /// `Exclusive`: a materialized view's concurrent refresh.
    #[strum(serialize = "ExclusiveLock")]
    Exclusive,
    /// `AccessExclusive`: a schema change.
    #[strum(serialize = "AccessExclusiveLock")]
    AccessExclusive,
}

impl WaitingMode {
    /// Whether a request in this mode conflicts with the guard's.
    const fn conflicts_with_guard(self) -> bool {
        match self {
            WaitingMode::AccessShare | WaitingMode::RowShare | WaitingMode::RowExclusive => false,
            WaitingMode::ShareUpdateExclusive | WaitingMode::Share | WaitingMode::ShareRowExclusive | WaitingMode::Exclusive | WaitingMode::AccessExclusive => {
                true
            }
        }
    }
}

/// Where the guard's request stands in the table's lock queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueState {
    /// The request is not queued yet.
    NotYetQueued,
    /// The request is queued, and a conflicting request waits beside it,
    /// queued before it.
    ConflictAhead,
    /// The request is queued, and nothing that conflicts with it waits.
    GuardFirst,
}

/// How the setup ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SetupEnd {
    /// The setup committed with the guard next in line.
    Committed,
    /// The setup rolled back to yield to a conflicting request.
    Yielded,
}

/// Where the guard stands, from the backends waiting on the table.
fn queue_state(waiters: &[(i32, WaitingMode)], guard_pid: i32) -> QueueState {
    if !waiters.iter().any(|&(pid, _)| pid == guard_pid) {
        return QueueState::NotYetQueued;
    }
    if waiters.iter().any(|&(pid, mode)| pid != guard_pid && mode.conflicts_with_guard()) {
        QueueState::ConflictAhead
    } else {
        QueueState::GuardFirst
    }
}

/// Configure the guard's session and read its backend.
pub(crate) async fn prepare_guard(connection: &mut PgConnection) -> Result<i32, MaintenanceError> {
    SettingAssignment::new(Setting::IdleInTransactionSessionTimeout, "0", SettingScope::Session)
        .to_query()
        .execute(&mut *connection)
        .await?;
    Ok(Query::select()
        .expr(backend_pid())
        .to_owned()
        .to_query_scalar::<i32>()
        .fetch_one(connection)
        .await?)
}

/// Queue the guard's lock on `table` behind the setup, then commit the setup
/// once the guard is next in line, or roll it back to yield. The guard's
/// transaction holds its lock once this returns [`SetupEnd::Committed`].
pub(crate) async fn commit_setup_behind_guard(
    setup: Transaction<'_, Postgres>,
    guard: &mut PgConnection,
    guard_pid: i32,
    table: &TableName,
    relid: u32,
) -> Result<SetupEnd, MaintenanceError> {
    // The lock command owns the relations it names; the run keeps naming the
    // table after the guard's request.
    let request = LockAttempt::new(vec![table.clone()], LockMode::ShareUpdateExclusive, QUEUE_BUDGET);
    let guarded = async { request.to_query().execute(&mut *guard).await.map(drop) };
    let (guarded, ended) = tokio::join!(guarded, end_setup(setup, guard_pid, relid));
    let ended = ended?;
    match ended {
        SetupEnd::Committed => guarded?,
        SetupEnd::Yielded => {}
    }
    Ok(ended)
}

/// Wait for the guard's request in the queue, then commit the setup, or roll
/// it back when a conflicting request is ahead or the request never shows.
async fn end_setup(mut setup: Transaction<'_, Postgres>, guard_pid: i32, relid: u32) -> Result<SetupEnd, MaintenanceError> {
    let started = Instant::now();
    loop {
        let waiters = waiting_requests(&mut setup, relid).await?;
        match queue_state(&waiters, guard_pid) {
            QueueState::GuardFirst => {
                setup.commit().await?;
                return Ok(SetupEnd::Committed);
            }
            QueueState::ConflictAhead => {
                setup.rollback().await?;
                return Ok(SetupEnd::Yielded);
            }
            QueueState::NotYetQueued if started.elapsed() >= QUEUE_BUDGET => {
                setup.rollback().await?;
                return Ok(SetupEnd::Yielded);
            }
            QueueState::NotYetQueued => time::sleep(QUEUE_POLL).await,
        }
    }
}

/// Every request of this database waiting on the table `relid`, with its
/// mode.
async fn waiting_requests(connection: &mut PgConnection, relid: u32) -> Result<Vec<(i32, WaitingMode)>, MaintenanceError> {
    let rows = waiting_statement(relid).to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let mode: String = row.try_get(1)?;
            Ok((row.try_get(0)?, WaitingMode::from_str(&mode).map_err(MaintenanceError::UnknownLockMode)?))
        })
        .collect()
}

/// The waiting requests on the table `relid` in this database.
fn waiting_statement(relid: u32) -> SelectStatement {
    let current_database_oid = Query::select()
        .column(Alias::new("oid"))
        .from(Alias::new("pg_database"))
        .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
        .to_owned();
    Query::select()
        .columns([Alias::new("pid"), Alias::new("mode")])
        .from(Alias::new("pg_locks"))
        .and_where(Expr::col(Alias::new("locktype")).eq("relation"))
        .and_where(Expr::col(Alias::new("database")).in_subquery(current_database_oid))
        .and_where(Expr::col(Alias::new("relation")).eq(cast(Expr::val(i64::from(relid)), PostgresType::Oid)))
        .and_where(Expr::col(Alias::new("granted")).eq(false))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::repack::guard_session::{QueueState, WaitingMode, queue_state, waiting_statement};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    /// The spellings `pg_locks.mode` reports, as the lock manager names its
    /// table-level modes.
    #[test]
    fn every_mode_is_read_as_pg_locks_spells_it() {
        let modes = [
            "AccessShareLock",
            "RowShareLock",
            "RowExclusiveLock",
            "ShareUpdateExclusiveLock",
            "ShareLock",
            "ShareRowExclusiveLock",
            "ExclusiveLock",
            "AccessExclusiveLock",
        ];
        assert_eq!(
            modes.map(|mode| WaitingMode::from_str(mode).ok()),
            WaitingMode::iter().map(Some).collect::<Vec<_>>().as_slice()
        );
        assert!(WaitingMode::from_str("AccessShare").is_err());
    }
    use sea_query::PostgresQueryBuilder;

    const GUARD: i32 = 101;

    #[test]
    fn the_setup_waits_until_the_guard_is_queued() {
        assert_eq!(queue_state(&[(300, WaitingMode::RowExclusive)], GUARD), QueueState::NotYetQueued);
    }

    #[test]
    fn writers_queued_beside_the_guard_let_the_setup_commit() {
        let waiters = [
            (300, WaitingMode::RowExclusive),
            (GUARD, WaitingMode::ShareUpdateExclusive),
            (301, WaitingMode::AccessShare),
        ];
        assert_eq!(queue_state(&waiters, GUARD), QueueState::GuardFirst);
    }

    #[test]
    fn a_waiting_schema_change_or_vacuum_makes_the_setup_yield() {
        for mode in [WaitingMode::AccessExclusive, WaitingMode::ShareUpdateExclusive, WaitingMode::Share] {
            let waiters = [(GUARD, WaitingMode::ShareUpdateExclusive), (400, mode)];
            assert_eq!(queue_state(&waiters, GUARD), QueueState::ConflictAhead, "{mode:?}");
        }
    }

    #[test]
    fn the_queue_is_read_from_this_databases_waiting_requests_on_the_table() {
        let sql = waiting_statement(16_384).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT "pid", "mode" FROM "pg_locks""#), "{sql}");
        assert!(sql.contains(r#""relation" = (16384::oid) AND "granted" = FALSE"#), "{sql}");
    }
}
