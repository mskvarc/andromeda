//! The transaction and wall clock functions.
//!
//! A write stamps `modified_at` with the instant the transaction began, so
//! every row one statement touches carries the same time. PostgreSQL's `NOW()`
//! is that instant (transaction-start, not statement- or call-time), and
//! sea-query has no constructor for it. A lease check inside a long
//! transaction needs the opposite: the instant the check actually runs, which
//! is `CLOCK_TIMESTAMP()`.

use sea_query::{Func, Iden, SimpleExpr};

/// The `NOW()` transaction-clock function.
#[derive(Iden, Clone, Copy)]
#[iden = "NOW"]
struct Now;

/// The `CLOCK_TIMESTAMP()` wall-clock function.
#[derive(Iden, Clone, Copy)]
#[iden = "CLOCK_TIMESTAMP"]
struct ClockTimestamp;

/// PostgreSQL: `NOW()` - the instant the current transaction began.
///
/// Every row a set-write touches reads the same value, since it is fixed for the
/// transaction rather than re-evaluated per row.
#[must_use]
pub fn now() -> SimpleExpr {
    Func::cust(Now).into()
}

/// PostgreSQL: `CLOCK_TIMESTAMP()` - the instant the expression is evaluated.
///
/// Unlike [`now`], it keeps advancing inside a transaction, so a lease compared
/// against it late in a long transaction is compared against the time the
/// comparison runs, not the time the transaction began.
#[must_use]
pub fn wall_clock() -> SimpleExpr {
    Func::cust(ClockTimestamp).into()
}

#[cfg(test)]
mod tests {
    use crate::clock::{now, wall_clock};
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn now_renders_the_transaction_clock_call() {
        let sql = Query::select().expr(now()).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("NOW()"), "{sql}");
    }

    #[test]
    fn wall_clock_renders_the_evaluation_time_call() {
        let sql = Query::select().expr(wall_clock()).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("CLOCK_TIMESTAMP()"), "{sql}");
    }
}
