//! The judgement of a Scope assertion: whether the Scope one write states the
//! Entity is in is already the Scope in force at the write.
//!
//! Clause 4.18 makes a Scope valid "from the time it has been set until the
//! time it has been explicitly removed by an update or delete operation". A
//! write that only restates the Scope in force sets nothing, so its assertion
//! records an event only where the state in force at its instant differs from
//! the paths it states. A state is the set of paths of the events positioned
//! at one instant, empty after a removal, which is how the clause 4.19
//! reading of the history groups them too.
//!
//! The state is compared on two axes. A broker-dated scope event carries
//! `created_at = modified_at = recorded_at`, so the `createdAt` and
//! `modifiedAt` axes order every event exactly as `recorded_at` does; only
//! `observed_at` departs from it, for a reified instance the client dated.
//! An assertion is redundant when the state at the greatest `recorded_at` at
//! or before its instant and the state at the greatest `observed_at` at or
//! before it both hold exactly its paths.
//!
//! One judgement serves both readers of an assertion: the intent application
//! that records it, judging it against the recorded events and the earlier
//! entries of its batch, and the temporal read of the live branch that reads
//! it while it is queued, judging it against the recorded events and the
//! earlier queued entries. Each says which events the judgement reads; the
//! comparison itself is written once, here.

use andromeda_postgresql_schema::temporal::ScopeHistory;
use andromeda_postgresql_seaquery::{
    collation::{PostgresCollation, collate},
    pg_array::array_of_subquery,
};
use sea_query::{Condition, Expr, ExprTrait, Iden, Order, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};

/// The alias a state's events are read under, `sas`.
#[derive(Iden, Clone, Copy)]
#[iden = "sas"]
struct StateEvent;

/// The alias the probe for a state's instant reads under, `sah`.
#[derive(Iden, Clone, Copy)]
#[iden = "sah"]
struct HeadEvent;

/// The axes an assertion is judged on.
const JUDGED_AXES: [ScopeHistory; 2] = [ScopeHistory::RecordedAt, ScopeHistory::ObservedAt];

/// The columns a relation of scope events projects for a judgement, in order:
/// the event's write instant, its `observedAt`, and its path, NULL for a
/// removal.
pub const EVENT_COLUMNS: [ScopeHistory; 3] = [ScopeHistory::RecordedAt, ScopeHistory::ObservedAt, ScopeHistory::VerbatimScopePath];

/// The set of paths `paths` selects, one row each from the expression `path`,
/// as a `text[]` sorted and compared byte-wise.
///
/// Both sides of a judgement are built here, so they agree on the order and
/// on the collation whatever relation each reads: a recorded path is
/// `COLLATE "C"`, a queued one carries the default collation.
#[must_use]
pub fn path_set(mut paths: SelectStatement, path: SimpleExpr) -> SimpleExpr {
    // Paths compare by byte identity, the one clause 4.18 spelling comparison
    // every Scope uses.
    let byte_order = PostgresCollation::byte_order();
    // clone: the path is both selected and ordered by.
    paths.expr(path.clone()).order_by_expr(collate(path, &byte_order), Order::Asc);
    collate(array_of_subquery(paths), &byte_order)
}

/// Whether the paths `asserted` holds, a [`path_set`], are the state in force
/// at `at` on every axis among the scope events `events` projects for one
/// Entity, its columns those of [`EVENT_COLUMNS`].
///
/// An Entity with no event at or before `at` on an axis holds no state there,
/// which no assertion is.
#[must_use]
pub fn held_in_force(events: &SelectStatement, asserted: &SimpleExpr, at: &SimpleExpr) -> Condition {
    JUDGED_AXES.into_iter().fold(Condition::all(), |held, axis| {
        // clone: each axis compares the same assertion.
        held.add(state_at(events, axis, at).eq(asserted.clone()))
    })
}

/// The state in force at `at` on `axis`: the paths of the events positioned at
/// the greatest instant at or before it.
fn state_at(events: &SelectStatement, axis: ScopeHistory, at: &SimpleExpr) -> SimpleExpr {
    let mut head = Query::select();
    head.column((HeadEvent, axis))
        // clone: the head probe and the state read the same events.
        .from_subquery(events.clone(), HeadEvent)
        // clone: each axis bounds its probe by the same instant.
        .and_where(Expr::col((HeadEvent, axis)).lte(at.clone()))
        .order_by((HeadEvent, axis), Order::Desc)
        .limit(1);
    let mut state = Query::select();
    state
        // clone: the head probe and the state read the same events.
        .from_subquery(events.clone(), StateEvent)
        .and_where(Expr::col((StateEvent, ScopeHistory::VerbatimScopePath)).is_not_null())
        .and_where(Expr::col((StateEvent, axis)).eq(SimpleExpr::SubQuery(None, Box::new(head.into_sub_query_statement()))));
    path_set(state, Expr::col((StateEvent, ScopeHistory::VerbatimScopePath)))
}

#[cfg(test)]
mod tests {
    use crate::scope_assertion::{EVENT_COLUMNS, held_in_force, path_set};
    use andromeda_postgresql_schema::temporal::ScopeHistory;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement};

    fn events() -> SelectStatement {
        Query::select()
            .columns(EVENT_COLUMNS)
            .from(ScopeHistory::Table)
            .and_where(Expr::col(ScopeHistory::EntityKey).eq(Expr::col((Alias::new("te"), Alias::new("entity_key")))))
            .to_owned()
    }

    fn rendered() -> String {
        let asserted = path_set(
            Query::select().from(Alias::new("asserted")).to_owned(),
            Expr::col(Alias::new("verbatim_scope_path")),
        );
        Query::select()
            .expr(Expr::val(1))
            .cond_where(held_in_force(&events(), &asserted, &Expr::col(Alias::new("at"))))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// The head on each axis is the newest event at or before the assertion's
    /// instant, one descent of the Entity's index on that axis.
    #[test]
    fn each_axis_reads_its_newest_event_at_or_before_the_instant() {
        let sql = rendered();
        for axis in ["recorded_at", "observed_at"] {
            assert!(
                sql.contains(&format!(
                    r#"SELECT "sah"."{axis}" FROM (SELECT "recorded_at", "observed_at", "verbatim_scope_path" FROM "scope_history""#
                )),
                "{axis}: {sql}"
            );
            assert!(
                sql.contains(&format!(r#"WHERE "sah"."{axis}" <= "at" ORDER BY "sah"."{axis}" DESC LIMIT 1"#)),
                "{axis}: {sql}"
            );
        }
        assert!(!sql.contains("created_at") && !sql.contains("modified_at"), "{sql}");
    }

    /// A state is the set of paths at the head instant, a removal naming none,
    /// and is compared with the assertion as a set sorted byte-wise.
    #[test]
    fn a_state_is_compared_with_the_assertion_as_a_byte_ordered_path_set() {
        let sql = rendered();
        assert!(sql.contains(r#""sas"."verbatim_scope_path" IS NOT NULL"#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY ("sas"."verbatim_scope_path") COLLATE "C" ASC)) COLLATE "C") = ((ARRAY (SELECT "verbatim_scope_path" FROM "asserted" ORDER BY ("verbatim_scope_path") COLLATE "C" ASC)) COLLATE "C")"#),
            "{sql}"
        );
        assert_eq!(sql.matches(r#"= ((ARRAY (SELECT "verbatim_scope_path" FROM "asserted""#).count(), 2, "{sql}");
    }
}
