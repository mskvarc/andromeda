//! The `attribute_history` column each Temporal Property is filtered, bucketed
//! and ordered on, and the bounds a stream read on it carries.
//!
//! Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and `deletedAt`
//! as a `timeproperty`, and clause 4.11 makes `observedAt` the default. Every
//! axis is stored on its own column and read from that column alone: a row that
//! carries no value on the requested axis is outside every bounded window on it,
//! and the operational `recorded_at` never stands in for a missing one.
//!
//! Each axis has a stream-led index holding the rows that carry it, keyed on
//! the column and then on the write that recorded the row, so a read ordered
//! and bounded on the column is a range of that index. The bound on the column
//! is also what excludes a columnstore chunk by its range on the axis.

use crate::time_interval_predicate::within_interval;
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use sea_query::{Condition, Expr, ExprTrait, IntoIden, SimpleExpr};

/// The `attribute_history` column that holds `timeproperty`.
#[must_use]
pub const fn axis_column(timeproperty: TimeProp) -> AttributeHistory {
    match timeproperty {
        TimeProp::Observed => AttributeHistory::ObservedAt,
        TimeProp::Created => AttributeHistory::CreatedAt,
        TimeProp::Modified => AttributeHistory::ModifiedAt,
        TimeProp::Deleted => AttributeHistory::DeletedAt,
    }
}

/// The key a stream's rows under `row` are ordered on for `timeproperty`: the
/// instant on its column.
#[must_use]
pub fn axis_order(row: impl IntoIden, timeproperty: TimeProp) -> SimpleExpr {
    Expr::col((row.into_iden(), axis_column(timeproperty)))
}

/// The rows under `row` that carry an instant on `timeproperty`, which is the
/// predicate of the axis's stream-led index, so a read the window leaves
/// unbounded still reads that index.
#[must_use]
pub fn axis_present(row: impl IntoIden, timeproperty: TimeProp) -> Condition {
    Condition::all().add(Expr::expr(axis_order(row, timeproperty)).is_not_null())
}

/// `compare` of the instant of the row under `row` on `timeproperty`.
#[must_use]
pub fn axis_bound(row: impl IntoIden, timeproperty: TimeProp, compare: impl Fn(SimpleExpr) -> SimpleExpr) -> SimpleExpr {
    compare(axis_order(row, timeproperty))
}

/// The rows under `row` whose instant on `timeproperty` lies within
/// `interval`.
#[must_use]
pub fn axis_window(row: impl IntoIden, timeproperty: TimeProp, interval: &TimeInterval) -> Condition {
    within_interval(&axis_order(row, timeproperty), interval)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::history_source::axis::{axis_bound, axis_column, axis_order, axis_present, axis_window};
    use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, ExprTrait, Iden, Order, PostgresQueryBuilder, Query, SelectStatement};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn ah() -> Alias {
        Alias::new("ah")
    }

    fn rendered(build: impl FnOnce(&mut SelectStatement)) -> String {
        let mut select = Query::select();
        build(&mut select);
        select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn each_time_property_maps_to_its_own_history_column() {
        let expected = [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ];
        for (prop, column) in expected {
            pretty_assertions::assert_eq!(Iden::to_string(&axis_column(prop)), column);
        }
    }

    #[test]
    fn no_axis_resolves_to_the_operational_write_clock() {
        for prop in [TimeProp::Observed, TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            assert_ne!(Iden::to_string(&axis_column(prop)), "recorded_at", "{prop}");
        }
    }

    /// Every axis is ordered on its own column, which its stream-led index
    /// keys.
    #[test]
    fn a_stream_is_ordered_on_the_column_of_its_axis() {
        for (prop, key) in [
            (TimeProp::Observed, r#"ORDER BY "ah"."observed_at" ASC"#),
            (TimeProp::Created, r#"ORDER BY "ah"."created_at" ASC"#),
            (TimeProp::Modified, r#"ORDER BY "ah"."modified_at" ASC"#),
            (TimeProp::Deleted, r#"ORDER BY "ah"."deleted_at" ASC"#),
        ] {
            let sql = rendered(|select| {
                select.order_by_expr(axis_order(ah(), prop), Order::Asc);
            });
            assert!(sql.ends_with(key), "{prop}: {sql}");
        }
    }

    /// A bound names the column alone, so a deletion carrying no instant on
    /// the axis is never inside it.
    #[test]
    fn a_bound_on_an_axis_bounds_its_column_alone() {
        let at = instant("2026-01-01T00:00:00Z");
        for (prop, column) in [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Deleted, "deleted_at"),
        ] {
            let sql = rendered(|select| {
                select.and_where(axis_bound(ah(), prop, |instant| instant.gt(at)));
            });
            let expected = format!(r#"WHERE "ah"."{column}" > '2026-01-01 00:00:00.000000 +00:00'"#);
            assert!(sql.ends_with(&expected), "{prop}: {sql}");
            assert!(!sql.contains("COALESCE"), "{prop}: {sql}");
        }
    }

    /// A window bounds the column from each side.
    #[test]
    fn a_window_on_an_axis_bounds_its_column_from_each_side() {
        let interval = TimeInterval::Between {
            from: instant("2026-01-01T00:00:00Z"),
            to: instant("2026-02-01T00:00:00Z"),
        };
        for (prop, column) in [(TimeProp::Observed, "observed_at"), (TimeProp::Modified, "modified_at")] {
            let sql = rendered(|select| {
                select.cond_where(axis_window(ah(), prop, &interval));
            });
            let expected = format!(r#"WHERE "ah"."{column}" >= '2026-01-01 00:00:00.000000 +00:00' AND "ah"."{column}" < '2026-02-01 00:00:00.000000 +00:00'"#);
            assert!(sql.ends_with(&expected), "{prop}: {sql}");
        }
    }

    /// An unbounded window still states the predicate of the axis's partial
    /// index, which keeps a row without the axis out.
    #[test]
    fn presence_on_an_axis_is_stated_on_its_column() {
        for (prop, column) in [(TimeProp::Observed, "observed_at"), (TimeProp::Deleted, "deleted_at")] {
            let sql = rendered(|select| {
                select.cond_where(axis_present(ah(), prop));
            });
            assert!(sql.ends_with(&format!(r#"WHERE "ah"."{column}" IS NOT NULL"#)), "{prop}: {sql}");
        }
    }

    /// Each axis's stream-led index holds only the rows carrying the axis.
    /// `observedAt` and `deletedAt` key the column and then the write that
    /// recorded the row, descending in the order a `lastN` reads; `createdAt`
    /// and `modifiedAt` key the write instant alone, which every instance of
    /// one write shares, ascending in the order a stream receives it.
    #[test]
    fn the_migration_indexes_each_axis_by_its_column_over_the_rows_carrying_it() {
        let migration = include_str!("../../../../migrations/branch/0018_attribute_history.sql");
        for (column, key) in [
            ("observed_at", "observed_at desc, write_id desc, write_ordinal desc"),
            ("created_at", "created_at"),
            ("modified_at", "modified_at"),
            ("deleted_at", "deleted_at desc, write_id desc, write_ordinal desc"),
        ] {
            let index =
                format!("create index idx_attribute_history_stream_{column} on attribute_history (stream_key, {key})\nwhere\n    {column} is not null;");
            assert!(migration.contains(&index), "the migration declares {index}");
        }
    }
}
