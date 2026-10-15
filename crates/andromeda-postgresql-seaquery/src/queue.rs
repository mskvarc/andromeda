//! The claimable-instant expression the durable queues order and filter on.
//!
//! Every lease-based queue in the schema answers the same question at claim
//! time: is this row due, and is nobody holding it? Two columns carry that —
//! the instant the row becomes due (`next_attempt_at`, `available_at`) and
//! the instant the current lease expires (`lease_until`, `locked_until`, null
//! when unleased) — and a row is claimable when the later of the two has
//! passed. Folding both into one expression turns a two-column predicate the
//! planner can only half-serve from an index into a single ordered key that an
//! expression index covers completely: `WHERE claimable_at <= now ORDER BY
//! claimable_at` becomes one bounded range scan whose depth is the batch size.
//!
//! The expression is spelled here once so the statement and the migration that
//! defines the matching expression index agree exactly. PostgreSQL matches an
//! expression index structurally, so the `-infinity` sentinel that stands in
//! for "unleased" is rendered as a typed literal rather than bound as a
//! parameter: a bound value only matches under a custom plan, and a generic
//! plan would silently stop using the index.

use crate::open_instant::or_start_of_time;
use sea_query::{Func, SimpleExpr};

/// PostgreSQL: `GREATEST(due, COALESCE(lease_until, '-infinity'::timestamptz))`
/// — the instant from which a queue row may be claimed.
///
/// `due` is the column holding the earliest instant the row may be attempted
/// and `lease_until` the column holding the current lease's expiry, null when
/// no lease is held. Both are passed as expressions so a statement can name its
/// own table alias. `-infinity` is below every real instant, so an unleased
/// row is claimable exactly when it is due.
#[must_use]
pub fn claimable_at(due: SimpleExpr, lease_until: SimpleExpr) -> SimpleExpr {
    Func::greatest([due, or_start_of_time(lease_until)]).into()
}

#[cfg(test)]
mod tests {
    use crate::queue::claimable_at;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_claimable_instant_is_the_later_of_due_and_lease_expiry_with_unleased_as_minus_infinity() {
        let sql = Query::select()
            .expr(claimable_at(Expr::col(Alias::new("next_attempt_at")), Expr::col(Alias::new("lease_until"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"GREATEST("next_attempt_at", COALESCE("lease_until", '-infinity'::timestamptz))"#),
            "{sql}"
        );
    }
}
