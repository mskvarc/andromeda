//! The claimable-instant expression the durable queues order and filter on,
//! and the predicate that bounds which rows one claim leases.
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
//!
//! A claim is one `UPDATE` that leases the rows a locked, limited selection
//! picks. The selection reaches the update only through [`is_claimed_from`],
//! so every queue leases at most the rows its limit admits.

use crate::{
    open_instant::or_start_of_time,
    pg_array::{any, array_of_subquery},
};
use sea_query::{ExprTrait, Func, SelectStatement, SimpleExpr};

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

/// PostgreSQL: `key = ANY(ARRAY (candidates))` — the rows a claim leases are
/// exactly the keys its selection picked.
///
/// `candidates` is the queue's own selection of one key column, ordered,
/// limited to the batch and locked `FOR UPDATE SKIP LOCKED`. As
/// `key IN (candidates)` the planner may choose a semi join that rescans the
/// selection once per row of the updated table, and each rescan's row lock
/// steps over the rows the same command has already updated, so one claim
/// leases every due row instead of at most the limit. An uncorrelated
/// `ARRAY (…)` is an init plan: the selection runs once and its keys are fixed
/// before the update reads a row, whatever plan the update gets.
#[must_use]
pub fn is_claimed_from(key: impl Into<SimpleExpr>, candidates: SelectStatement) -> SimpleExpr {
    key.into().eq(any(array_of_subquery(candidates)))
}

#[cfg(test)]
mod tests {
    use crate::queue::{claimable_at, is_claimed_from};
    use sea_query::{Alias, Expr, ExprTrait, LockBehavior, LockType, PostgresQueryBuilder, Query};

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

    #[test]
    fn a_claim_leases_the_keys_its_locked_limited_selection_picked_once() {
        let candidates = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("queue"))
            .and_where(Expr::col(Alias::new("due")).lte(Expr::current_timestamp()))
            .limit(1)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .to_owned();
        let (sql, values) = Query::update()
            .table(Alias::new("queue"))
            .value(Alias::new("claimed"), true)
            .and_where(is_claimed_from(Expr::col(Alias::new("id")), candidates))
            .to_owned()
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"UPDATE "queue" SET "claimed" = $1 WHERE "id" = ANY(ARRAY (SELECT "id" FROM "queue" WHERE "due" <= CURRENT_TIMESTAMP LIMIT $2 FOR UPDATE SKIP LOCKED))"#
        );
        pretty_assertions::assert_eq!(values.0.len(), 2);
    }
}
