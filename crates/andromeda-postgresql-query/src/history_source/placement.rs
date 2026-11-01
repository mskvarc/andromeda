//! Where an attribute history row stands on a Temporal Property's axis.
//!
//! A row is placed at its instant on the axis and a deletion that carries none
//! at the instant it was deleted (clause 4.5.7), so a row is placed at
//! `COALESCE(axis, deleted_at)` and a row with neither is placed nowhere. The
//! stream-led placement indexes key exactly that expression, which is why
//! every statement spells it through here.

use andromeda_postgresql_schema::temporal::AttributeHistory;
use sea_query::{Expr, Func, IntoIden, SimpleExpr};

/// `COALESCE(axis, deleted_at)` of the row under `row`, written as the
/// placement indexes declare it so the planner matches them to it.
#[must_use]
pub fn placement(row: impl IntoIden, axis: AttributeHistory) -> SimpleExpr {
    let row = row.into_iden();
    // clone: each column reference takes the alias by value.
    Func::coalesce([Expr::col((row.clone(), axis)), Expr::col((row, AttributeHistory::DeletedAt))]).into()
}

#[cfg(test)]
mod tests {
    use crate::history_source::placement::placement;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    /// The expression is the one the placement indexes key, column for
    /// column, so the planner matches an index to a bound or an order on it.
    #[test]
    fn a_row_is_placed_at_its_axis_instant_or_else_its_deletion() {
        let sql = Query::select()
            .expr(placement(Alias::new("rh"), AttributeHistory::CreatedAt))
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT COALESCE("rh"."created_at", "rh"."deleted_at")"#);
    }

    /// Each placement index keys the expression over its own axis.
    #[test]
    fn the_migration_indexes_each_axis_by_the_same_expression() {
        let migration = include_str!("../../../../migrations/branch/0018_attribute_history.sql");
        for column in ["observed_at", "created_at", "modified_at"] {
            let key = format!("(stream_key, (coalesce({column}, deleted_at)) desc)");
            assert!(migration.contains(&key), "the migration keys {key}");
        }
    }
}
