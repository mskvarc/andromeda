//! A repack's replay, swap and cleanup.
//!
//! The log is replayed until it has converged and every transaction older
//! than the copy has ended. The guard then takes `ACCESS EXCLUSIVE` on the
//! table and the copy in bounded attempts, with the log replayed between
//! them, replays what is left and gives the table the copy's files; the
//! caller commits the guard's transaction, or cleans up on it when the lock
//! was not granted. Writes committed while the lock was awaited can leave
//! more in the log than a short replay: the lock is then given back, the log
//! replayed down again and the lock tried anew, so the table is never held
//! for longer than [`SWAP_LOG_ROWS`] rows take to replay. The old
//! files, now in the copy table, and the run's other objects are dropped
//! under the table's lock, and the run's row with them.

use crate::{
    error::MaintenanceError,
    repack::{
        RepackAbandonment,
        lock_acquisition::{self, BetweenAttempts, LockOutcome},
        log_replay::{self, ReplayStep, ReplayWatch},
        old_transactions::{self, OldTransactions, OldTransactionsStep},
        table_script::RepackTableScript,
    },
};
use andromeda_postgresql_seaquery::{
    relation_lock::LockMode,
    repack::{drop_objects, swap},
    statement::Statement,
    transaction::{Isolation, TransactionStart},
};
use sea_query::{Alias, TableName};
use sqlx::{Acquire, PgConnection};
use std::{
    slice,
    time::{Duration, Instant},
};
use tokio::time;

/// How often the replay looks again while it waits for old transactions.
const OLD_TRANSACTIONS_POLL: Duration = Duration::from_secs(1);

/// The most log rows the swap replays under the table's lock.
const SWAP_LOG_ROWS: u64 = 1_000;

/// Whether the swap may replay what the log holds under its lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SwapReplay {
    /// The log is short enough to replay under the lock.
    UnderLock,
    /// The log grew while the lock was awaited; it is replayed first.
    GiveLockBack,
}

/// Whether `remaining` log rows may be replayed under the swap's lock.
const fn swap_replay(remaining: u64) -> SwapReplay {
    if remaining <= SWAP_LOG_ROWS {
        SwapReplay::UnderLock
    } else {
        SwapReplay::GiveLockBack
    }
}

/// How a phase that may give up ended.
pub(crate) enum PhaseEnd {
    /// The phase completed.
    Done,
    /// The run is abandoned.
    Abandoned(RepackAbandonment),
}

/// Replay the log onto the copy until it has converged and the transactions
/// in `old` have ended.
pub(crate) async fn replay_until_converged(main: &mut PgConnection, script: &RepackTableScript, old: &OldTransactions) -> Result<PhaseEnd, MaintenanceError> {
    let started = Instant::now();
    let mut watch = ReplayWatch::new();
    let mut waiting_since = None;
    loop {
        log_replay::apply_batch(&mut *main, &script.replay).await?;
        let remaining = log_replay::remaining_rows(&mut *main, script.relid).await?;
        match watch.observe(remaining, started.elapsed()) {
            ReplayStep::Continue => continue,
            ReplayStep::NotConverging => return Ok(PhaseEnd::Abandoned(RepackAbandonment::NotConverging)),
            ReplayStep::Converged => {}
        }
        let since = *waiting_since.get_or_insert_with(Instant::now);
        let alive = old_transactions::still_open(&mut *main, old).await?;
        match old_transactions::old_transactions_step(alive, since.elapsed()) {
            OldTransactionsStep::Gone => return Ok(PhaseEnd::Done),
            OldTransactionsStep::Waiting => time::sleep(OLD_TRANSACTIONS_POLL).await,
            OldTransactionsStep::TimedOut => return Ok(PhaseEnd::Abandoned(RepackAbandonment::NotConverging)),
        }
    }
}

/// Give `table` the copy's files in the guard's transaction `guard`,
/// replaying the log on `main` between lock attempts. Each try at the lock
/// runs in a savepoint, whose rollback gives the lock back when the log has
/// outgrown a short replay. An abandoned swap leaves the guard's transaction
/// open, holding its `SHARE UPDATE EXCLUSIVE` lock.
pub(crate) async fn swap_files(
    guard: &mut PgConnection,
    main: &mut PgConnection,
    script: &RepackTableScript,
    table: &TableName,
) -> Result<PhaseEnd, MaintenanceError> {
    // The lock command owns the relations it names; the run keeps naming the
    // table for the cleanup.
    let relations = [table.clone(), copy_table(script.relid)];
    let started = Instant::now();
    let mut watch = ReplayWatch::new();
    loop {
        let mut attempt = guard.begin().await?;
        let between = BetweenAttempts {
            connection: &mut *main,
            script: &script.replay,
        };
        if lock_acquisition::acquire(&mut attempt, &relations, LockMode::AccessExclusive, Some(between)).await? == LockOutcome::Abandoned {
            attempt.rollback().await?;
            return Ok(PhaseEnd::Abandoned(RepackAbandonment::LockWait));
        }
        match swap_replay(log_replay::remaining_rows(&mut attempt, script.relid).await?) {
            SwapReplay::UnderLock => {
                log_replay::apply_all(&mut attempt, &script.replay).await?;
                swap(script.relid).to_query().execute(&mut *attempt).await?;
                attempt.commit().await?;
                return Ok(PhaseEnd::Done);
            }
            SwapReplay::GiveLockBack => attempt.rollback().await?,
        }
        loop {
            log_replay::apply_batch(&mut *main, &script.replay).await?;
            let remaining = log_replay::remaining_rows(&mut *main, script.relid).await?;
            match watch.observe(remaining, started.elapsed()) {
                ReplayStep::Converged => break,
                ReplayStep::Continue => {}
                ReplayStep::NotConverging => return Ok(PhaseEnd::Abandoned(RepackAbandonment::NotConverging)),
            }
        }
    }
}

/// Drop the run's objects, the old files among them, under the table's lock.
pub(crate) async fn drop_old_files(main: &mut PgConnection, relid: u32, table: &TableName) -> Result<PhaseEnd, MaintenanceError> {
    let mut drop = TransactionStart::at(Isolation::ReadCommitted).begin_on(main).await?;
    if lock_acquisition::acquire(&mut drop, slice::from_ref(table), LockMode::AccessExclusive, None).await? == LockOutcome::Abandoned {
        drop.rollback().await?;
        return Ok(PhaseEnd::Abandoned(RepackAbandonment::LockWait));
    }
    drop_objects(relid).to_query().execute(&mut *drop).await?;
    drop.commit().await?;
    Ok(PhaseEnd::Done)
}

/// The run's copy table of the table `relid`.
fn copy_table(relid: u32) -> TableName {
    TableName::from((Alias::new("repack"), Alias::new(format!("table_{relid}"))))
}

#[cfg(test)]
mod tests {
    use crate::repack::swap::{SwapReplay, swap_replay};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_short_log_is_replayed_under_the_lock() {
        assert_eq!(swap_replay(0), SwapReplay::UnderLock);
        assert_eq!(swap_replay(1_000), SwapReplay::UnderLock);
    }

    /// A bulk write committed while the lock was awaited would hold every
    /// writer of the table for as long as its rows take to replay.
    #[test]
    fn a_log_grown_past_a_short_replay_gives_the_lock_back() {
        assert_eq!(swap_replay(1_001), SwapReplay::GiveLockBack);
        assert_eq!(swap_replay(560_000), SwapReplay::GiveLockBack);
    }
}
