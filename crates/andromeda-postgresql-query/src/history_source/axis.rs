//! The `attribute_history` column each Temporal Property is filtered, bucketed
//! and ordered on, and the bounds a stream read on it carries.
//!
//! Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and `deletedAt`
//! as a `timeproperty`, and clause 4.11 makes `observedAt` the default. Every
//! axis is stored on its own column and read from that column alone: a row that
//! carries no value on the requested axis is outside every bounded window on it,
//! and the operational `recorded_at` never stands in for a missing one.
//!
//! A stream's rows are read on `observedAt`, `createdAt` and `modifiedAt`
//! through the stream-led placement index of the axis rather than an index of
//! the axis column alone: the broker stamps `createdAt` and `modifiedAt` on
//! every instance (clause 4.8), so an index of either column would repeat its
//! placement for every row but a deletion's, and the placement index of
//! `observedAt` serves both the window and the instance in effect when it
//! opens. A read orders on the placement and bounds both the column and the
//! placement. The two agree on every row that carries an instant on the axis,
//! and the bound on the column keeps out a deletion placed at its `deletedAt`,
//! so a read returns the same rows it would on the column alone. The bound on
//! the placement is what the index serves; the bound on the column is what
//! excludes a columnstore chunk by its range on the axis, which no bound on an
//! expression does. `deletedAt` places every row on its own column.

use crate::{history_source::placement::placement, time_interval_predicate::within_interval};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use sea_query::{Condition, DynIden, Expr, ExprTrait, IntoIden, SimpleExpr};

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

/// The axis column of the row under `row`.
fn column_of(row: &DynIden, timeproperty: TimeProp) -> SimpleExpr {
    // clone: each column reference takes the alias by value.
    Expr::col((row.clone(), axis_column(timeproperty)))
}

/// The placement of the row under `row` on `timeproperty` when a stream is
/// read on it through the axis's placement index, `None` on `deletedAt`.
fn indexed_placement(row: &DynIden, timeproperty: TimeProp) -> Option<SimpleExpr> {
    match timeproperty {
        // clone: the placement names the alias in both of its columns.
        TimeProp::Observed | TimeProp::Created | TimeProp::Modified => Some(placement(row.clone(), axis_column(timeproperty))),
        TimeProp::Deleted => None,
    }
}

/// The key a stream's rows under `row` are ordered on for `timeproperty`.
#[must_use]
pub fn axis_order(row: impl IntoIden, timeproperty: TimeProp) -> SimpleExpr {
    let row = row.into_iden();
    indexed_placement(&row, timeproperty).unwrap_or_else(|| column_of(&row, timeproperty))
}

/// The rows under `row` that carry an instant on `timeproperty`, stated on the
/// placement too where the stream is read through the placement index, so a
/// read the window leaves unbounded still meets the predicate of a partial
/// placement index.
#[must_use]
pub fn axis_present(row: impl IntoIden, timeproperty: TimeProp) -> Condition {
    let row = row.into_iden();
    let column = Condition::all().add(Expr::expr(column_of(&row, timeproperty)).is_not_null());
    match indexed_placement(&row, timeproperty) {
        Some(placed) => column.add(Expr::expr(placed).is_not_null()),
        None => column,
    }
}

/// `compare` of the instant of the row under `row` on `timeproperty`, and of
/// its placement where the stream is read through the placement index.
#[must_use]
pub fn axis_bound(row: impl IntoIden, timeproperty: TimeProp, compare: impl Fn(SimpleExpr) -> SimpleExpr) -> SimpleExpr {
    let row = row.into_iden();
    let column = compare(column_of(&row, timeproperty));
    match indexed_placement(&row, timeproperty) {
        Some(placed) => column.and(compare(placed)),
        None => column,
    }
}

/// The rows under `row` whose instant on `timeproperty` lies within
/// `interval`, bounded as [`axis_bound`] bounds them.
#[must_use]
pub fn axis_window(row: impl IntoIden, timeproperty: TimeProp, interval: &TimeInterval) -> Condition {
    let row = row.into_iden();
    let column = within_interval(&column_of(&row, timeproperty), interval);
    match indexed_placement(&row, timeproperty) {
        Some(placed) => column.add(within_interval(&placed, interval)),
        None => column,
    }
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

    /// Every axis but `deletedAt` is ordered on the placement its stream-led
    /// index keys; `deletedAt` on its column.
    #[test]
    fn a_stream_is_ordered_on_the_placement_of_an_indexed_axis_and_on_the_column_of_deleted_at() {
        for (prop, key) in [
            (TimeProp::Observed, r#"ORDER BY COALESCE("ah"."observed_at", "ah"."deleted_at") ASC"#),
            (TimeProp::Created, r#"ORDER BY COALESCE("ah"."created_at", "ah"."deleted_at") ASC"#),
            (TimeProp::Modified, r#"ORDER BY COALESCE("ah"."modified_at", "ah"."deleted_at") ASC"#),
            (TimeProp::Deleted, r#"ORDER BY "ah"."deleted_at" ASC"#),
        ] {
            let sql = rendered(|select| {
                select.order_by_expr(axis_order(ah(), prop), Order::Asc);
            });
            assert!(sql.ends_with(key), "{prop}: {sql}");
        }
    }

    /// A bound keeps the bound on the column, which excludes columnstore
    /// chunks and a deletion placed at its `deletedAt`, beside the one on the
    /// placement, which the placement index serves.
    #[test]
    fn a_bound_on_an_indexed_axis_bounds_both_the_column_and_the_placement() {
        let at = instant("2026-01-01T00:00:00Z");
        for (prop, column) in [(TimeProp::Observed, "observed_at"), (TimeProp::Created, "created_at")] {
            let sql = rendered(|select| {
                select.and_where(axis_bound(ah(), prop, |instant| instant.gt(at)));
            });
            let expected = format!(
                r#"WHERE "ah"."{column}" > '2026-01-01 00:00:00.000000 +00:00' AND COALESCE("ah"."{column}", "ah"."deleted_at") > '2026-01-01 00:00:00.000000 +00:00'"#
            );
            assert!(sql.ends_with(&expected), "{prop}: {sql}");
        }
        let deleted = rendered(|select| {
            select.and_where(axis_bound(ah(), TimeProp::Deleted, |instant| instant.gt(at)));
        });
        assert!(
            deleted.ends_with(r#"WHERE "ah"."deleted_at" > '2026-01-01 00:00:00.000000 +00:00'"#),
            "{deleted}"
        );
    }

    /// A window on an indexed axis bounds both from each side.
    #[test]
    fn a_window_on_an_indexed_axis_bounds_both_the_column_and_the_placement_from_each_side() {
        let interval = TimeInterval::Between {
            from: instant("2026-01-01T00:00:00Z"),
            to: instant("2026-02-01T00:00:00Z"),
        };
        for (prop, column) in [(TimeProp::Observed, "observed_at"), (TimeProp::Modified, "modified_at")] {
            let sql = rendered(|select| {
                select.cond_where(axis_window(ah(), prop, &interval));
            });
            for bound in [
                format!(r#""ah"."{column}" >= '2026-01-01 00:00:00.000000 +00:00'"#),
                format!(r#""ah"."{column}" < '2026-02-01 00:00:00.000000 +00:00'"#),
                format!(r#"COALESCE("ah"."{column}", "ah"."deleted_at") >= '2026-01-01 00:00:00.000000 +00:00'"#),
                format!(r#"COALESCE("ah"."{column}", "ah"."deleted_at") < '2026-02-01 00:00:00.000000 +00:00'"#),
            ] {
                assert!(sql.contains(&bound), "{prop} {bound}: {sql}");
            }
        }
        let deleted = rendered(|select| {
            select.cond_where(axis_window(ah(), TimeProp::Deleted, &interval));
        });
        assert!(!deleted.contains("COALESCE"), "{deleted}");
    }

    /// An unbounded window on an indexed axis still states the placement,
    /// which the partial `observedAt` placement index requires, beside the
    /// column, which keeps a row without the axis out.
    #[test]
    fn presence_on_an_indexed_axis_is_stated_on_both_the_column_and_the_placement() {
        let sql = rendered(|select| {
            select.cond_where(axis_present(ah(), TimeProp::Observed));
        });
        assert!(
            sql.ends_with(r#"WHERE "ah"."observed_at" IS NOT NULL AND COALESCE("ah"."observed_at", "ah"."deleted_at") IS NOT NULL"#),
            "{sql}"
        );
        let deleted = rendered(|select| {
            select.cond_where(axis_present(ah(), TimeProp::Deleted));
        });
        assert!(deleted.ends_with(r#"WHERE "ah"."deleted_at" IS NOT NULL"#), "{deleted}");
    }
}
