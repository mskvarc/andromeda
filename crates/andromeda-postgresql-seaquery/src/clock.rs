//! The transaction and wall clock functions, and the instants a statement
//! sets from them.
//!
//! A write stamps `modified_at` with the instant the transaction began, so
//! every row one statement touches carries the same time. PostgreSQL's `NOW()`
//! is that instant (transaction-start, not statement- or call-time), and
//! sea-query has no constructor for it. A lease check inside a long
//! transaction needs the opposite: the instant the check actually runs, which
//! is `CLOCK_TIMESTAMP()`.
//!
//! A lease deadline or a retry instant a statement stores is set on the same
//! clock the claims compare it with, `CURRENT_TIMESTAMP` plus a span. An
//! instant computed on the broker's clock would move the deadline by however
//! far that clock is from the database's, which a broker on another host
//! always is by some amount.

use crate::interval::make_interval_secs;
use sea_query::{Expr, ExprTrait, Func, Iden, SimpleExpr};
use std::time::Duration;

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

/// PostgreSQL: `CURRENT_TIMESTAMP + make_interval(secs => seconds)`: the
/// instant `seconds` after the current transaction began, on the database
/// clock.
///
/// A transaction recording a failed attempt begins after that attempt ended,
/// so a retry it schedules waits its whole span after the failure.
#[must_use]
pub fn after_transaction_start(seconds: SimpleExpr) -> SimpleExpr {
    Expr::current_timestamp().add(make_interval_secs(seconds))
}

/// [`after_transaction_start`] of `span`, its length bound as fractional
/// seconds and never spelled into the statement.
#[must_use]
pub fn span_after_transaction_start(span: Duration) -> SimpleExpr {
    after_transaction_start(Expr::val(span.as_secs_f64()))
}

#[cfg(test)]
mod tests {
    use crate::clock::{after_transaction_start, now, span_after_transaction_start, wall_clock};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
    use std::time::Duration;

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

    #[test]
    fn a_span_is_added_to_the_database_clock_as_bound_fractional_seconds() {
        let (sql, values) = Query::select()
            .expr(span_after_transaction_start(Duration::from_millis(1_500)))
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, "SELECT CURRENT_TIMESTAMP + (make_interval(secs => $1))");
        pretty_assertions::assert_eq!(values.0, vec![1.5_f64.into()]);
    }

    #[test]
    fn a_wait_held_in_a_column_is_added_to_the_database_clock() {
        let sql = Query::select()
            .expr(after_transaction_start(Expr::col(Alias::new("retry_after"))))
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT CURRENT_TIMESTAMP + (make_interval(secs => "retry_after"))"#);
    }
}
