//! The `attribute_history` column each Temporal Property is filtered, bucketed
//! and ordered on.
//!
//! Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and `deletedAt`
//! as a `timeproperty`, and clause 4.11 makes `observedAt` the default. Every
//! axis is stored on its own column and read from that column alone: a row that
//! carries no value on the requested axis is outside every bounded window on it,
//! and the operational `recorded_at` never stands in for a missing one.
//!
//! A stream's rows are read on `createdAt` and `modifiedAt` through the
//! stream-led placement index of the axis, which holds every row, rather than
//! an index of the axis column alone: the broker stamps both on every
//! instance (clause 4.8), so such an index would repeat the placement for
//! every row but a deletion's. A read orders on the placement and bounds both
//! the column and the placement. The two agree on every represented instance,
//! which carries an instant on the axis. The bound on the placement is what
//! the index serves; the bound on the column is what excludes a columnstore
//! chunk by its range on the axis, which no bound on an expression does.

use crate::alias::Ah;
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use andromeda_postgresql_query::{history_source::placement::placement, time_interval_predicate::within_interval};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use sea_query::{Condition, Expr, ExprTrait, SimpleExpr};

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

/// The placement of the row under `ah` on `timeproperty` when a stream is
/// read on it through the axis's placement index, `None` when it is read
/// through an index of the axis column.
fn indexed_placement(timeproperty: TimeProp) -> Option<SimpleExpr> {
    match timeproperty {
        TimeProp::Created | TimeProp::Modified => Some(placement(Ah, axis_column(timeproperty))),
        TimeProp::Observed | TimeProp::Deleted => None,
    }
}

/// The key a stream's rows under `ah` are ordered on for `timeproperty`.
#[must_use]
pub(crate) fn axis_order(timeproperty: TimeProp) -> SimpleExpr {
    indexed_placement(timeproperty).unwrap_or_else(|| Expr::col((Ah, axis_column(timeproperty))))
}

/// `compare` of the instant of the row under `ah` on `timeproperty`, and of
/// its placement where the stream is read through the placement index.
#[must_use]
pub(crate) fn axis_bound(timeproperty: TimeProp, compare: impl Fn(SimpleExpr) -> SimpleExpr) -> SimpleExpr {
    let column = compare(Expr::col((Ah, axis_column(timeproperty))));
    match indexed_placement(timeproperty) {
        Some(placed) => column.and(compare(placed)),
        None => column,
    }
}

/// The rows under `ah` whose instant on `timeproperty` lies within
/// `interval`, bounded as [`axis_bound`] bounds them.
#[must_use]
pub(crate) fn axis_window(timeproperty: TimeProp, interval: &TimeInterval) -> Condition {
    let column = within_interval(&Expr::col((Ah, axis_column(timeproperty))), interval);
    match indexed_placement(timeproperty) {
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
    use crate::axis::{axis_bound, axis_column, axis_order, axis_window};
    use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use chrono::{DateTime, Utc};
    use sea_query::{ExprTrait, Iden, Order, PostgresQueryBuilder, Query, SelectStatement};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
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

    /// On the two axes the broker stamps, a stream is ordered on the
    /// placement its index keys; on the others on the axis column.
    #[test]
    fn a_stream_is_ordered_on_the_placement_of_a_stamped_axis_and_the_column_of_the_others() {
        for (prop, key) in [
            (TimeProp::Created, r#"ORDER BY COALESCE("ah"."created_at", "ah"."deleted_at") ASC"#),
            (TimeProp::Modified, r#"ORDER BY COALESCE("ah"."modified_at", "ah"."deleted_at") ASC"#),
            (TimeProp::Observed, r#"ORDER BY "ah"."observed_at" ASC"#),
            (TimeProp::Deleted, r#"ORDER BY "ah"."deleted_at" ASC"#),
        ] {
            let sql = rendered(|select| {
                select.order_by_expr(axis_order(prop), Order::Asc);
            });
            assert!(sql.ends_with(key), "{prop}: {sql}");
        }
    }

    /// A bound on a stamped axis keeps the bound on the column, which
    /// excludes columnstore chunks, beside the one on the placement, which
    /// the placement index serves.
    #[test]
    fn a_bound_on_a_stamped_axis_bounds_both_the_column_and_the_placement() {
        let at = instant("2026-01-01T00:00:00Z");
        let sql = rendered(|select| {
            select.and_where(axis_bound(TimeProp::Created, |instant| instant.gt(at)));
        });
        assert!(
            sql.ends_with(r#"WHERE "ah"."created_at" > '2026-01-01 00:00:00.000000 +00:00' AND COALESCE("ah"."created_at", "ah"."deleted_at") > '2026-01-01 00:00:00.000000 +00:00'"#),
            "{sql}"
        );
        let observed = rendered(|select| {
            select.and_where(axis_bound(TimeProp::Observed, |instant| instant.gt(at)));
        });
        assert!(
            observed.ends_with(r#"WHERE "ah"."observed_at" > '2026-01-01 00:00:00.000000 +00:00'"#),
            "{observed}"
        );
    }

    /// A window on a stamped axis bounds both from each side; a window on
    /// `observedAt` bounds the column alone.
    #[test]
    fn a_window_on_a_stamped_axis_bounds_both_the_column_and_the_placement_from_each_side() {
        let interval = TimeInterval::Between {
            from: instant("2026-01-01T00:00:00Z"),
            to: instant("2026-02-01T00:00:00Z"),
        };
        let sql = rendered(|select| {
            select.cond_where(axis_window(TimeProp::Modified, &interval));
        });
        for bound in [
            r#""ah"."modified_at" >= '2026-01-01 00:00:00.000000 +00:00'"#,
            r#""ah"."modified_at" < '2026-02-01 00:00:00.000000 +00:00'"#,
            r#"COALESCE("ah"."modified_at", "ah"."deleted_at") >= '2026-01-01 00:00:00.000000 +00:00'"#,
            r#"COALESCE("ah"."modified_at", "ah"."deleted_at") < '2026-02-01 00:00:00.000000 +00:00'"#,
        ] {
            assert!(sql.contains(bound), "{bound}: {sql}");
        }
        let observed = rendered(|select| {
            select.cond_where(axis_window(TimeProp::Observed, &interval));
        });
        assert!(!observed.contains("COALESCE"), "{observed}");
    }
}
