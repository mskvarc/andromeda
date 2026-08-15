//! The transaction clock function.
//!
//! A write stamps `modified_at` with the instant the transaction began, so
//! every row one statement touches carries the same time. PostgreSQL's `NOW()`
//! is that instant — transaction-start, not statement- or call-time — and
//! sea-query has no constructor for it.

use sea_query::{Func, Iden, SimpleExpr};

/// The `NOW()` transaction-clock function.
#[derive(Iden, Clone, Copy)]
#[iden = "NOW"]
struct Now;

/// PostgreSQL: `NOW()` - the instant the current transaction began.
///
/// Every row a set-write touches reads the same value, since it is fixed for the
/// transaction rather than re-evaluated per row.
pub fn now() -> SimpleExpr {
    Func::cust(Now).into()
}

#[cfg(test)]
mod tests {
    use crate::clock::now;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn now_renders_the_transaction_clock_call() {
        let sql = Query::select().expr(now()).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("NOW()"), "{sql}");
    }
}
