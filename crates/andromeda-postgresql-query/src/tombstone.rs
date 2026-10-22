//! The history rows that record a deletion.
//!
//! Clause 4.5.7 keeps a deleted attribute in its temporal evolution as an
//! instance with no value, recorded at the instant of the deletion: an
//! `attribute_history` row whose value kind is the tombstone, and a queued
//! entry of the tombstone kind until the queue is applied. Such an instance is
//! never a target value of a `q` or a geo-query, never aggregated and never
//! carries a value shape, and every statement that tells it apart says so
//! here, so no two statements can disagree on what a deletion is. The codes
//! are the broker's own constants, so they render inline and shift no
//! placeholder of the statement they join.

use andromeda_postgresql_codes::{temporal_entry_kind::TemporalEntryKind, value_kind::ValueKind};
use andromeda_postgresql_schema::{temporal::AttributeHistory, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Expr, ExprTrait, IntoIden, SimpleExpr};

/// Whether the attribute history row under `row` carries a value, which is
/// whether it is not a tombstone.
#[must_use]
pub fn carries_a_value(row: impl IntoIden) -> SimpleExpr {
    Expr::col((row.into_iden(), AttributeHistory::ValueKind)).ne(int_literal(i64::from(ValueKind::Tombstone.code())))
}

/// Whether the queued entry under `entry` carries a value, which is whether it
/// is not a tombstone.
#[must_use]
pub fn entry_carries_a_value(entry: impl IntoIden) -> SimpleExpr {
    Expr::col((entry.into_iden(), TemporalEntry::EntryKind)).ne(int_literal(i64::from(TemporalEntryKind::Tombstone.code())))
}

#[cfg(test)]
mod tests {
    use crate::tombstone::{carries_a_value, entry_carries_a_value};
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn a_history_row_carries_a_value_unless_its_value_kind_is_the_tombstone() {
        let (sql, values) = Query::select()
            .and_where(carries_a_value(Alias::new("ah")))
            .to_owned()
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT  WHERE "ah"."value_kind" <> (0)"#);
        pretty_assertions::assert_eq!(values.0, Vec::new());
    }

    #[test]
    fn a_queued_entry_carries_a_value_unless_its_kind_is_the_tombstone() {
        let (sql, values) = Query::select()
            .and_where(entry_carries_a_value(Alias::new("pe")))
            .to_owned()
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT  WHERE "pe"."entry_kind" <> (2)"#);
        pretty_assertions::assert_eq!(values.0, Vec::new());
    }
}
