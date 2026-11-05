//! Replaying the writes logged while a repack copies its table.
//!
//! The trigger logs every write the table takes after the setup, and the
//! extension's replay function applies logged rows to the copy in order. A
//! run replays in batches until the log holds fewer than [`SWITCH_ROWS`]
//! rows, the point where the swap's final replay, under `ACCESS EXCLUSIVE`,
//! is short. A log that writes keep from shrinking for [`APPLY_BUDGET`] never
//! gets there, and the run is abandoned rather than left logging every
//! write.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    repack::table_script::ReplayScript,
};
use andromeda_postgresql_seaquery::{repack::apply_log, statement::Statement};
use sea_query::{Alias, Expr, Func, Query};
use sqlx::PgConnection;
use std::time::Duration;

/// The rows one replay call applies.
const BATCH_ROWS: i32 = 1_000;

/// The log size under which a run moves on to the swap.
const SWITCH_ROWS: u64 = 100;

/// How long the log may go without shrinking below its smallest size.
const APPLY_BUDGET: Duration = Duration::from_mins(10);

/// What one observation of the log's size means for the replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplayStep {
    /// The log is small enough to swap.
    Converged,
    /// The log is still large and has shrunk within the budget.
    Continue,
    /// The log has not shrunk below its smallest size for the whole budget.
    NotConverging,
}

/// The smallest the log has been, and when it got there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReplayWatch {
    /// The smallest remaining row count seen.
    smallest: Option<u64>,
    /// When it was seen, measured from the replay's start.
    smallest_at: Duration,
}

impl ReplayWatch {
    /// A watch before the first observation.
    pub(crate) const fn new() -> ReplayWatch {
        ReplayWatch {
            smallest: None,
            smallest_at: Duration::ZERO,
        }
    }

    /// Observe `remaining` log rows `at` into the replay.
    pub(crate) fn observe(&mut self, remaining: u64, at: Duration) -> ReplayStep {
        if remaining < SWITCH_ROWS {
            return ReplayStep::Converged;
        }
        if self.smallest.is_none_or(|smallest| remaining < smallest) {
            self.smallest = Some(remaining);
            self.smallest_at = at;
            return ReplayStep::Continue;
        }
        if at.saturating_sub(self.smallest_at) >= APPLY_BUDGET {
            ReplayStep::NotConverging
        } else {
            ReplayStep::Continue
        }
    }
}

/// Replay one batch of logged rows onto the copy.
pub(crate) async fn apply_batch(connection: &mut PgConnection, script: &ReplayScript) -> Result<(), sqlx::Error> {
    apply_log(script.statements(), BATCH_ROWS).to_query().execute(connection).await.map(drop)
}

/// Replay every logged row onto the copy, the swap's final replay.
pub(crate) async fn apply_all(connection: &mut PgConnection, script: &ReplayScript) -> Result<(), sqlx::Error> {
    apply_log(script.statements(), 0).to_query().execute(connection).await.map(drop)
}

/// The rows the log of the table `relid` holds.
pub(crate) async fn remaining_rows(connection: &mut PgConnection, relid: u32) -> Result<u64, MaintenanceError> {
    let statement = Query::select()
        .expr(Func::count(Expr::val(1)))
        .from((Alias::new("repack"), Alias::new(format!("log_{relid}"))))
        .to_owned();
    non_negative(statement.to_query_scalar::<i64>().fetch_one(connection).await?, CatalogCounter::ReplayedRows)
}

#[cfg(test)]
mod tests {
    use crate::repack::log_replay::{ReplayStep, ReplayWatch};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn replayed(observations: &[(u64, u64)]) -> Vec<ReplayStep> {
        let mut watch = ReplayWatch::new();
        observations
            .iter()
            .map(|&(remaining, seconds)| watch.observe(remaining, Duration::from_secs(seconds)))
            .collect()
    }

    #[test]
    fn a_shrinking_log_converges_once_under_a_hundred_rows() {
        assert_eq!(
            replayed(&[(5_000, 1), (3_000, 2), (1_200, 3), (80, 4)]),
            [ReplayStep::Continue, ReplayStep::Continue, ReplayStep::Continue, ReplayStep::Converged]
        );
    }

    #[test]
    fn a_log_growing_for_the_whole_budget_does_not_converge() {
        assert_eq!(
            replayed(&[(5_000, 0), (6_000, 300), (7_000, 600)]),
            [ReplayStep::Continue, ReplayStep::Continue, ReplayStep::NotConverging]
        );
    }

    #[test]
    fn an_empty_log_converges_at_once() {
        assert_eq!(replayed(&[(0, 0)]), [ReplayStep::Converged]);
    }
}
