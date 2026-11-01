//! The periods of one stream's page on a calendar grid, aggregated over the
//! periods the page's edge listed (clause 4.5.19).
//!
//! Placing an instant on a calendar grid costs several interval products, and
//! the edge's probes have already placed every period of the page that holds
//! an instance. The page therefore takes its periods from the edge's list and
//! places no row on the grid:
//!
//! - a page of whole periods aggregates each listed period by a read of its
//!   own, bounded on both sides, so the stream's axis index returns the
//!   period's rows and nothing groups or sorts them;
//! - a page keeping its last `lastN` instances, a count no single period
//!   holds, places each kept instance among the listed starts by a binary
//!   search, groups the instances by the position found, and joins each group
//!   to its listed period.

use crate::{
    aggregation::PeriodOrder,
    aggregation_grid::{BucketEnd, BucketStart},
    alias::Ah,
    period_edge::{listed_ends, listed_starts, stream_bound},
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_postgresql_query::history_source::axis::axis_bound;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    aggregate::{count_all, row_number},
    literal::int_literal,
    pg_array::{array_positions, width_bucket},
    unnest::unnest_of,
};
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement, SimpleExpr, WindowStatement};

/// Alias `page_period`: one period the page's edge listed.
#[derive(Iden, Clone, Copy)]
#[iden = "page_period"]
struct PagePeriod;

/// Column `period_position`: the position of a period in the edge's list.
#[derive(Iden, Clone, Copy)]
#[iden = "period_position"]
struct PeriodPosition;

/// Alias `period_aggregate`: the aggregates of one listed period.
#[derive(Iden, Clone, Copy)]
#[iden = "period_aggregate"]
struct PeriodAggregate;

/// The column an `aggrGroupBy` key is projected under so the page can number
/// its periods by it outside the aggregate.
fn group_order(index: usize) -> Alias {
    Alias::new(format!("group_order_{index}"))
}

/// The instances under `ah` within the listed period under [`PagePeriod`]:
/// from its start, before its end.
pub(crate) fn within_page_period(timeproperty: TimeProp) -> Condition {
    Condition::all()
        .add(axis_bound(Ah, timeproperty, |instant| instant.gte(Expr::col((PagePeriod, BucketStart)))))
        .add(axis_bound(Ah, timeproperty, |instant| instant.lt(Expr::col((PagePeriod, BucketEnd)))))
}

/// The periods the edge listed, one row each: its start, its end and its
/// position in the list, the set-returning functions advancing in step.
fn page_periods() -> SelectStatement {
    let mut periods = Query::select();
    periods
        .expr_as(unnest_of(listed_starts()), BucketStart)
        .expr_as(unnest_of(listed_ends()), BucketEnd)
        .expr_as(array_positions(listed_starts()), PeriodPosition);
    periods
}

/// The periods of a page of whole periods: each listed period before the
/// page's edge, beside the aggregates of `aggregate` over the rows its
/// condition [`within_page_period`] admits, `keys` its `aggrGroupBy` keys.
///
/// An aggregate without a group key answers one row even over no rows, and a
/// listed period whose instances are all deletions has none to aggregate, so
/// such a period is left out, as the grid leaves out a period with nothing to
/// aggregate.
pub(crate) fn each_listed_period(mut aggregate: SelectStatement, keys: Vec<SimpleExpr>) -> SelectStatement {
    if keys.is_empty() {
        aggregate.and_having(count_all().gt(int_literal(0)));
    }
    let order = ordered_by(&mut aggregate, &keys);
    aggregate.add_group_by(keys);
    let mut select = periods_beside(order);
    select
        .from_subquery(page_periods(), PagePeriod)
        .join_lateral(JoinType::Join, aggregate, PeriodAggregate, SimpleExpr::from(true))
        .and_where(Expr::col((PagePeriod, BucketStart)).lt(stream_bound()));
    select
}

/// The periods of a page keeping its last `lastN` instances: the aggregates
/// of `aggregate` grouped by `keys` and by the listed period each instance on
/// `axis` falls in, each group beside its period.
///
/// Every instance a backward page keeps lies in a period the probes found, so
/// the last listed start at or before its instant is its period's.
pub(crate) fn placed_among_listed_periods(mut aggregate: SelectStatement, mut keys: Vec<SimpleExpr>, axis: AttributeHistory) -> SelectStatement {
    let position = width_bucket(Expr::col((Ah, axis)), listed_starts());
    // clone: the position is projected and grouped by.
    aggregate
        .expr_as(position.clone(), PeriodPosition)
        .and_where(Expr::col((Ah, axis)).is_not_null());
    let order = ordered_by(&mut aggregate, &keys);
    keys.push(position);
    aggregate.add_group_by(keys);
    let mut select = periods_beside(order);
    select.from_subquery(aggregate, PeriodAggregate).join_subquery(
        JoinType::Join,
        page_periods(),
        PagePeriod,
        Expr::col((PagePeriod, PeriodPosition)).equals((PeriodAggregate, PeriodPosition)),
    );
    select
}

/// Project each of the `aggrGroupBy` `keys` of `aggregate` under
/// [`group_order`], returning the columns the page numbers its periods by
/// outside it.
fn ordered_by(aggregate: &mut SelectStatement, keys: &[SimpleExpr]) -> Vec<Alias> {
    let order: Vec<Alias> = (0..keys.len()).map(group_order).collect();
    for (key, alias) in keys.iter().zip(&order) {
        // clone: the key is projected here and grouped by in the aggregate, and the alias names the column outside it.
        aggregate.expr_as(key.clone(), alias.clone());
    }
    order
}

/// The projection of a listed period's row: its start and end, its
/// aggregates, and its position among the stream's periods, numbered by the
/// `aggrGroupBy` keys `order` names and then by its start.
fn periods_beside(order: Vec<Alias>) -> SelectStatement {
    let mut window = WindowStatement::new();
    for alias in order {
        window.order_by_expr(Expr::col((PeriodAggregate, alias)), Order::Asc);
    }
    window.order_by_expr(Expr::col((PagePeriod, BucketStart)), Order::Asc);
    let mut select = Query::select();
    select
        .expr_as(Expr::col((PagePeriod, BucketStart)), BucketStart)
        .expr_as(Expr::col((PagePeriod, BucketEnd)), BucketEnd)
        .expr(Expr::col((PeriodAggregate, Asterisk)))
        .expr_window_as(row_number(), window, PeriodOrder);
    select
}

#[cfg(test)]
mod tests {
    use crate::{
        alias::Ah,
        listed_period_page::{each_listed_period, placed_among_listed_periods, within_page_period},
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use andromeda_postgresql_seaquery::aggregate::count_all;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

    /// `SELECT COUNT(*) AS "agg_total_count" FROM "rows" AS "ah"`.
    fn aggregate() -> SelectStatement {
        Query::select()
            .expr_as(count_all(), Alias::new("agg_total_count"))
            .from_as(Alias::new("rows"), Ah)
            .to_owned()
    }

    fn unit_key() -> SimpleExpr {
        Expr::col((Ah, AttributeHistory::UnitCodeId))
    }

    /// Each listed period is one row of the unnested lists, aggregated by a
    /// lateral read, and only the periods before the page's edge are kept.
    #[test]
    fn each_listed_period_before_the_edge_is_aggregated_by_its_own_read() {
        let sql = each_listed_period(aggregate(), Vec::new()).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"FROM (SELECT UNNEST("stream_edge"."period_starts") AS "bucket_start", UNNEST("stream_edge"."period_ends") AS "bucket_end", generate_subscripts("stream_edge"."period_starts", 1) AS "period_position") AS "page_period""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"JOIN LATERAL (SELECT COUNT(*) AS "agg_total_count" FROM "rows" AS "ah""#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"WHERE "page_period"."bucket_start" < "stream_edge"."bound""#), "{sql}");
        assert!(
            sql.starts_with(r#"SELECT "page_period"."bucket_start" AS "bucket_start", "page_period"."bucket_end" AS "bucket_end", "period_aggregate".*"#),
            "{sql}"
        );
    }

    /// An aggregate without a group key answers one row over no rows, so a
    /// listed period holding only deletions answers none; with a group key the
    /// grouping alone answers no row over no rows.
    #[test]
    fn a_listed_period_with_nothing_to_aggregate_has_no_row() {
        let sql = each_listed_period(aggregate(), Vec::new()).to_string(PostgresQueryBuilder);
        assert!(sql.contains("HAVING (COUNT(*)) > (0)"), "{sql}");
        let grouped = each_listed_period(aggregate(), vec![unit_key()]).to_string(PostgresQueryBuilder);
        assert!(!grouped.contains("HAVING"), "{grouped}");
        assert!(grouped.contains(r#"GROUP BY "ah"."unit_code_id""#), "{grouped}");
    }

    /// The periods are numbered by the `aggrGroupBy` keys first, projected
    /// inside each period's aggregate, then by their start, so a group's
    /// periods stay contiguous.
    #[test]
    fn the_periods_are_numbered_by_their_group_keys_then_by_their_start() {
        let sql = each_listed_period(aggregate(), vec![unit_key()]).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""ah"."unit_code_id" AS "group_order_0""#), "{sql}");
        assert!(
            sql.contains(r#"ROW_NUMBER() OVER (  ORDER BY "period_aggregate"."group_order_0" ASC, "page_period"."bucket_start" ASC ) AS "period_order""#),
            "{sql}"
        );
    }

    /// A kept instance is placed by binary search among the listed starts, the
    /// instances are grouped by the position found and by the group keys, and
    /// each group is joined to the listed period at that position.
    #[test]
    fn a_kept_instance_is_placed_by_the_position_of_its_listed_start() {
        let sql = placed_among_listed_periods(aggregate(), vec![unit_key()], AttributeHistory::ObservedAt).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"width_bucket("ah"."observed_at", "stream_edge"."period_starts") AS "period_position""#),
            "{sql}"
        );
        assert!(sql.contains(r#"WHERE "ah"."observed_at" IS NOT NULL"#), "{sql}");
        assert!(
            sql.contains(r#"GROUP BY "ah"."unit_code_id", width_bucket("ah"."observed_at", "stream_edge"."period_starts")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ON "page_period"."period_position" = "period_aggregate"."period_position""#),
            "{sql}"
        );
        assert!(!sql.contains("HAVING"), "{sql}");
        assert!(!sql.contains(r#""stream_edge"."bound""#), "{sql}");
    }

    /// The rows of the listed period start at its start and stop before its
    /// end, the grid putting an instant on a boundary in the period that
    /// starts there.
    #[test]
    fn the_rows_of_a_listed_period_run_from_its_start_to_before_its_end() {
        let sql = Query::select()
            .expr(Expr::val(1))
            .cond_where(within_page_period(TimeProp::Observed))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.ends_with(concat!(
                r#"WHERE "ah"."observed_at" >= "page_period"."bucket_start" AND COALESCE("ah"."observed_at", "ah"."deleted_at") >= "page_period"."bucket_start" "#,
                r#"AND ("ah"."observed_at" < "page_period"."bucket_end" AND COALESCE("ah"."observed_at", "ah"."deleted_at") < "page_period"."bucket_end")"#
            )),
            "{sql}"
        );
    }
}
