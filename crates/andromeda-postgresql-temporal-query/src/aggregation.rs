//! Aggregated temporal values: the periods and the aggregate expressions of
//! clause 4.5.19.
//!
//! Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
//! period, so the statement groups on exactly those three and on nothing else.
//! Adding a fourth column to the group key would answer one period with two
//! entries whenever the rows underneath it disagreed about that column, which
//! is a shape the aggregated representation has no place to put.
//!
//! What the rows disagreed about is still a fact the reader needs. An aggregate
//! is also a statement about a unit: clause 4.5.2 lets every Property instance
//! carry its own `unitCode`, and nothing in the specification says what the mean
//! of a kilogram and a pound is, while Andromeda never converts. The unit identity and the two registry codes are
//! therefore projected as their own extrema over the group, and
//! [`crate::aggregation_group`] decides from that span whether the group is
//! answerable at all.
//!
//! Every attribute stream is aggregated on its own, in a LATERAL over the
//! Entity's streams, so the stream's axis index serves the read and a page of
//! whole periods ([`crate::aggregated_page`]) bounds it per stream. A page on
//! a calendar grid takes its periods from its edge
//! ([`crate::listed_period_page`]) rather than placing each row on the grid.

use crate::{
    aggregated_page::{StreamRows, aggregated_stream_read},
    aggregated_value::{arithmetic_numeric_expr, average_time_expr, distinct_value_expr, ordered_instant_expr, ordered_text_expr, sized_numeric_expr},
    aggregation_grid::{AggregationGrid, BucketEnd, BucketStart},
    aggregation_grouping::apply_grouping,
    alias::{Ah, An},
    listed_period_page::{each_listed_period, placed_among_listed_periods},
    period_edge::StreamEdge,
    recorded_shape::recorded_shape,
    stream_page::{StreamContinues, project_whole_streams},
    stream_parts::{St, entity_streams},
};
use andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, window::TimeInterval};
use andromeda_postgresql_query::{
    history_source::{HistoryBranch, HistoryOf, attribute_rows::attribute_rows, axis::axis_column},
    query_names::QueryNames,
};
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, temporal::AttributeHistory};
use andromeda_postgresql_seaquery::{
    aggregate::{bool_or, count_all, count_distinct, row_number, stddev},
    cast::cast,
    literal::int_literal,
    pg_type::PostgresType,
};
use andromeda_postgresql_stored_form::{default_instance::is_default_instance, history_grain::HistoryGrain, tombstone::carries_a_value};
use sea_query::{
    Alias,
    Asterisk,
    CommonTableExpression,
    Condition,
    Expr,
    ExprTrait,
    Func,
    Iden,
    IntoIden,
    JoinType,
    Order,
    Query,
    QueryStatementBuilder,
    SelectStatement,
    SimpleExpr,
    WindowStatement,
    WithClause,
};

/// Lateral projection that computes one period per history row.
#[derive(Iden, Clone, Copy)]
#[iden = "bucket"]
struct Bucket;

/// The CTE of the implicit per-entity period origin.
#[derive(Iden, Clone, Copy)]
#[iden = "bucket_origin"]
struct BucketOrigin;

/// Origin instant projected by [`BucketOrigin`].
#[derive(Iden, Clone, Copy)]
#[iden = "origin"]
struct Origin;

/// Column `period_origin`: the anchor the grid's periods are laid from, the
/// reference instant of the window or else the earliest instant of the history.
#[derive(Iden, Clone, Copy)]
#[iden = "period_origin"]
struct PeriodOrigin;

/// Alias `page`: the aggregated periods of one stream.
#[derive(Iden, Clone, Copy)]
#[iden = "page"]
struct Page;

/// Column `period_order`: the position of a period among its stream's, in
/// the order the representation lists them.
#[derive(Iden, Clone, Copy)]
#[iden = "period_order"]
pub(crate) struct PeriodOrder;

/// The span of registry codes and unit identities one aggregation group covered.
///
/// Every entry is an aggregate over the group rather than a group key, which is
/// what keeps the group key at the attribute, dataset and period clause 4.5.19
/// names. The family and the value shape live on the row rather than in the
/// catalogue, so an attribute whose type changed within a period is one period
/// whose rows disagree, not two periods. `MIN`/`MAX` skip nulls, so the unit
/// span also carries whether any row carried no unit at all.
///
/// The value shape is spanned at the grain the aggregation tables distinguish
/// rather than at the grain the row stores: an object and an array share one
/// JSONB column, and a group holding both has no single `min` even though every
/// row of it declared the same `value_kind`.
fn project_group_span(select: &mut SelectStatement) {
    select.expr_as(Func::min(Expr::col((Ah, AttributeHistory::AttributeKind))), Alias::new("attribute_kind_lowest"));
    select.expr_as(
        Func::max(Expr::col((Ah, AttributeHistory::AttributeKind))),
        Alias::new("attribute_kind_highest"),
    );
    select.expr_as(Func::min(recorded_shape()), Alias::new("recorded_shape_lowest"));
    select.expr_as(Func::max(recorded_shape()), Alias::new("recorded_shape_highest"));
    select.expr_as(Func::min(Expr::col((Ah, AttributeHistory::UnitCodeId))), Alias::new("unit_code_id_lowest"));
    select.expr_as(Func::max(Expr::col((Ah, AttributeHistory::UnitCodeId))), Alias::new("unit_code_id_highest"));
    select.expr_as(bool_or(Expr::col((Ah, AttributeHistory::UnitCodeId)).is_null()), Alias::new("any_unitless"));
}

/// The aggregate projections clause 4.5.19 asks for, one per method, plus the
/// typed columns `min`/`max` read from.
///
/// `avg`, `min`, `max` and `sum` fold the operand that measures an array by its
/// size, which is what Table 4.5.19.1-1 defines them over; `sumsq` and
/// `stddev`, which that table marks N/A for a JSON Array, fold the narrower
/// operand that never reaches the JSONB column. Every one of those operands is
/// gated on the attribute family, so a Relationship, `ListRelationship` or
/// `VocabProperty` (Table 4.5.19.1-3 and the "URI" column of Table 4.5.19.1-2
/// mark all six N/A for them) contributes to none of them. Only the two
/// counting methods read such a row.
///
/// Every statement counts the instances of each period, requested or not: a
/// backward page states the rest of its `lastN` from the instances its
/// periods aggregate (clause 6.3.10), and the representation writes
/// `totalCount` only when it was asked for.
fn aggregate_columns(methods: &[AggrMethod]) -> Vec<(SimpleExpr, &'static str)> {
    let mut columns: Vec<(SimpleExpr, &'static str)> = vec![(count_all(), "agg_total_count")];
    for method in methods {
        match method {
            AggrMethod::Avg => {
                columns.push((Func::avg(sized_numeric_expr()).into(), "agg_avg"));
                columns.push((average_time_expr(), "agg_avg_time"));
            }
            AggrMethod::Min => {
                columns.push((Func::min(sized_numeric_expr()).into(), "agg_min"));
                columns.push((Func::min(ordered_text_expr()).into(), "agg_min_str"));
                columns.push((Func::min(ordered_instant_expr()).into(), "agg_min_dt"));
            }
            AggrMethod::Max => {
                columns.push((Func::max(sized_numeric_expr()).into(), "agg_max"));
                columns.push((Func::max(ordered_text_expr()).into(), "agg_max_str"));
                columns.push((Func::max(ordered_instant_expr()).into(), "agg_max_dt"));
            }
            AggrMethod::Sum => columns.push((Func::sum(sized_numeric_expr()).into(), "agg_sum")),
            AggrMethod::SumSq => columns.push((Func::sum(arithmetic_numeric_expr().mul(arithmetic_numeric_expr())).into(), "agg_sumsq")),
            AggrMethod::StdDev => columns.push((stddev(arithmetic_numeric_expr()), "agg_stddev")),
            AggrMethod::TotalCount => {}
            AggrMethod::DistinctCount => columns.push((count_distinct(distinct_value_expr()), "agg_distinct_count")),
        }
    }
    columns
}

/// The bucket origin, measured over the history of the same entity rather than
/// through a current-state row.
///
/// The entity is the one stage 2 reads, so the origin is derived once and
/// supplied to the single bucket calculation rather than repeated in both
/// rendered bounds.
fn origin_subquery(branch: &HistoryBranch, of: &HistoryOf, axis: AttributeHistory) -> SelectStatement {
    let ah2 = Alias::new("ah2");
    let mut origin = Query::select();
    origin
        .expr_as(Func::min(Expr::col((ah2.clone(), axis))), Origin)
        .from(attribute_rows(branch, of, ah2));
    origin.take()
}

/// The origin of the period grid: the reference instant of the query's window
/// (clause 4.5.19), else the earliest instant of the Entity's history on the
/// axis, read once for the statement in the CTE the second element names,
/// rather than from epoch.
fn grid_origin(branch: &HistoryBranch, of: &HistoryOf, compiled: &CompiledTemporalQuery) -> (SimpleExpr, Option<CommonTableExpression>) {
    if let Some(origin) = compiled.window.interval.reference() {
        return (Expr::val(*origin), None);
    }
    let origin = CommonTableExpression::new()
        .query(origin_subquery(branch, of, axis_column(compiled.window.timeproperty)))
        .table_name(BucketOrigin)
        .materialized(true)
        .to_owned();
    let mut read = Query::select();
    read.column((BucketOrigin, Origin)).from(BucketOrigin);
    (SimpleExpr::SubQuery(None, Box::new(read.into_sub_query_statement())), Some(origin))
}

/// One bound of the whole-range period: the bound the query sets, or for a
/// side it leaves open the extreme instant the stream holds on the axis, the
/// least recent for an open start and the most recent for an open end
/// (clause 4.5.19.1).
fn whole_range_bound(bound: Option<SimpleExpr>, extreme: SimpleExpr) -> SimpleExpr {
    match bound {
        Some(instant) => cast(instant, PostgresType::Timestamptz),
        None => extreme,
    }
}

/// The aggregates of `rows` under `ah` clause 4.5.19 asks for, beside the span
/// of each group, and the `aggrGroupBy` keys the caller groups them by.
///
/// Clause 4.5.19: the fixed key is the attribute and `datasetId`, which the
/// stream is; the `aggrGroupBy` extension adds its own keys ahead of the
/// period.
fn aggregated_rows(
    rows: SelectStatement,
    compiled: &CompiledTemporalQuery,
    branch: &HistoryBranch,
    of: &HistoryOf,
    names: &QueryNames,
) -> (SelectStatement, Vec<SimpleExpr>) {
    let axis = axis_column(compiled.window.timeproperty);
    let mut select = Query::select();
    project_group_span(&mut select);
    select.from_subquery(rows, Ah);
    let keys: Vec<SimpleExpr> = match &compiled.representation.grouping {
        Some(grouping) => apply_grouping(&mut select, grouping, axis, branch, of, names),
        None => Vec::new(),
    };
    for (expr, alias) in aggregate_columns(&compiled.representation.aggr_methods) {
        select.expr_as(expr, Alias::new(alias));
    }
    (select, keys)
}

/// The periods of one stream's page, each with its aggregates, numbered in the
/// order the representation lists them.
fn stream_page(
    rows: StreamRows,
    grid: Option<&AggregationGrid>,
    compiled: &CompiledTemporalQuery,
    branch: &HistoryBranch,
    of: &HistoryOf,
    names: &QueryNames,
) -> SelectStatement {
    match rows {
        StreamRows::OnTheGrid(rows) => grouped_page(rows, grid, compiled, branch, of, names),
        StreamRows::AmongListedPeriods(rows) => {
            let (aggregate, keys) = aggregated_rows(rows, compiled, branch, of, names);
            placed_among_listed_periods(aggregate, keys, axis_column(compiled.window.timeproperty))
        }
        StreamRows::OfEachListedPeriod(rows) => {
            let (aggregate, keys) = aggregated_rows(rows, compiled, branch, of, names);
            each_listed_period(aggregate, keys)
        }
    }
}

/// The periods of one stream's `rows`, each row placed on `grid`, numbered in
/// the order the representation lists them.
///
/// An absent period is the whole range: one period rather than no grouping at
/// all.
fn grouped_page(
    rows: SelectStatement,
    grid: Option<&AggregationGrid>,
    compiled: &CompiledTemporalQuery,
    branch: &HistoryBranch,
    of: &HistoryOf,
    names: &QueryNames,
) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);
    let (mut select, mut keys) = aggregated_rows(rows, compiled, branch, of, names);
    let mut order = keys.clone();
    if let Some(grid) = grid {
        select
            .join_lateral(JoinType::Join, grid.period_of(Expr::col((Ah, axis))), Bucket, SimpleExpr::from(true))
            .expr_as(AggregationGrid::start_under(Bucket), BucketStart)
            .expr_as(grid.end_under(Bucket), BucketEnd);
        keys.extend(grid.keys_under(Bucket));
        order.push(AggregationGrid::start_under(Bucket));
    } else {
        let interval: TimeInterval = compiled.window.interval;
        select
            .expr_as(
                whole_range_bound(interval.opening().map(|instant| Expr::val(*instant)), Func::min(Expr::col((Ah, axis))).into()),
                BucketStart,
            )
            .expr_as(
                whole_range_bound(interval.closing().map(|instant| Expr::val(*instant)), Func::max(Expr::col((Ah, axis))).into()),
                BucketEnd,
            );
    }

    if keys.is_empty() {
        // An aggregate without a group key answers one row even over no rows;
        // a stream with nothing to aggregate has no period instead.
        select.and_having(count_all().gt(int_literal(0)));
    }
    select.add_group_by(keys);
    let mut window = WindowStatement::new();
    for key in order {
        window.order_by_expr(key, Order::Asc);
    }
    select.expr_window_as(row_number(), window, PeriodOrder);
    select
}

/// Build the aggregation statement over the streams of the Entity `of`
/// names, each stream aggregated on its own over the rows the compiled page
/// reads of it.
///
/// `condition` admits the represented instances. Clause 4.5.7: a tombstone
/// records a deletion and carries no value, so the rows that record one are
/// outside every aggregate rather than counted. On a grid every row also
/// projects the origin its periods were laid from as [`PeriodOrigin`].
#[must_use]
pub fn build_aggregation_query(
    branch: &HistoryBranch,
    of: &HistoryOf,
    compiled: &CompiledTemporalQuery,
    condition: &Condition,
    names: &QueryNames,
) -> SelectStatement {
    let (grid, origin, laid_from) = match compiled.representation.aggr_period_duration {
        Some(period) => {
            let (origin_expr, origin) = grid_origin(branch, of, compiled);
            (Some(AggregationGrid::new(period, origin_expr.clone())), origin, Some(origin_expr))
        }
        None => (None, None, None),
    };
    // clone: the aggregates fold only the valued instances, so they take the condition narrowed by carries_a_value.
    let aggregated = condition.clone().add(carries_a_value(Ah));
    let read = aggregated_stream_read(branch, compiled, condition, &aggregated, grid.as_ref());

    let mut select = Query::select();
    select
        .expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"))
        .expr_as(Expr::col((St, HistoryGrain::DatasetKey)), HistoryGrain::DatasetKey)
        .expr(Expr::col((Page, Asterisk)));
    if let Some(laid_from) = laid_from {
        select.expr_as(cast(laid_from, PostgresType::Timestamptz), PeriodOrigin);
    }
    entity_streams(&mut select, branch, of);
    match read.edge {
        Some(edge) => {
            select
                .expr_as(Expr::col((StreamEdge, StreamContinues)), StreamContinues)
                .join_lateral(JoinType::Join, edge, StreamEdge, SimpleExpr::from(true));
        }
        None => project_whole_streams(&mut select),
    }
    select.join_lateral(
        JoinType::Join,
        stream_page(read.rows, grid.as_ref(), compiled, branch, of, names),
        Page,
        SimpleExpr::from(true),
    );
    if let Some(origin) = origin {
        select.with_cte(WithClause::new().cte(origin).to_owned());
    }
    select.take()
}

/// Order the aggregated rows under `rows` as the representation lists them:
/// by attribute, each attribute's named instances before its default one,
/// then each stream's periods in order.
pub(crate) fn in_listing_order(select: &mut SelectStatement, rows: impl IntoIden) {
    let rows = rows.into_iden();
    select
        .order_by((rows.clone(), Alias::new("attribute_name")), Order::Asc)
        // The named instances of an attribute come before its default
        // instance, whose grain key, the empty string, sorts first.
        .order_by_expr(is_default_instance(Expr::col((rows.clone(), HistoryGrain::DatasetKey))), Order::Asc)
        .order_by((rows.clone(), HistoryGrain::DatasetKey), Order::Asc)
        .order_by((rows, PeriodOrder), Order::Asc);
}

#[cfg(test)]
mod tests {
    use crate::{aggregation::build_aggregation_query, history::build_temporal_history_query};
    use andromeda_ngsi_ld_lexicon::{
        aggr_method::AggrMethod,
        identifier::Identifier,
        temporal_format::TemporalFormat,
        time::date_time::NgsiLdDateTime,
        time_prop::TimeProp,
    };
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{
            aggregation_period::AggregationPeriod,
            compiled::CompiledTemporalQuery,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{
        history_source::{HistoryBranch, HistoryOf},
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use chrono::{DateTime, Utc};
    use sea_query::{Condition, Expr, SelectStatement};
    use std::slice;

    fn aggregated(methods: Vec<AggrMethod>, period: Option<&str>) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::AggregatedValues)
                    .aggr_methods(methods)
                    .aggr_period_duration(period.and_then(|raw| AggregationPeriod::of_parameter(raw).expect("valid period")))
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn grouped(methods: Vec<AggrMethod>, period: Option<&str>, aggr_group_by: &str) -> CompiledTemporalQuery {
        use andromeda_ngsi_ld_query::temporal::grouping::aggregation_grouping::AggregationGrouping;
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::GroupedValues)
                    .aggr_methods(methods)
                    .aggr_period_duration(period.and_then(|raw| AggregationPeriod::of_parameter(raw).expect("valid period")))
                    .grouping(Some(aggr_group_by.parse::<AggregationGrouping>().expect("a well-formed grouping")))
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    /// The entity the aggregation reads the history of.
    fn vehicle() -> HistoryOf {
        HistoryOf::Entity(Expr::val("urn:ngsi-ld:Vehicle:1"))
    }

    fn statement_of(compiled: &CompiledTemporalQuery) -> SelectStatement {
        build_aggregation_query(&HistoryBranch::live(), &vehicle(), compiled, &Condition::all(), &QueryNames::default())
    }

    #[test]
    fn a_period_produces_a_bucketed_group_by() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!("aggregation-a-period-produces-a-bucketed-group-by", statement);
    }

    #[test]
    fn the_bound_statement_projects_one_bucket_expression_and_reuses_its_result() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!(
            "aggregation-the-bound-statement-projects-one-bucket-expression-and-reuses-its-result",
            statement
        );
    }

    /// Clause 4.5.19: the bucket is computed on the same axis the window filters
    /// on, and clause 4.11 makes that `observedAt` by default, so a backfilled
    /// observation lands in the period its filter selected. The correlated origin
    /// subquery measures the same axis under its own alias.
    #[test]
    fn observed_at_buckets_on_the_observation_axis_not_write_time() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!("aggregation-observed-at-buckets-on-the-observation-axis-not-write-time", statement);
    }

    /// The bucket origin is measured over the same entity's streams, reached
    /// through the stream catalogue rather than through a current-state row.
    #[test]
    fn the_bucket_origin_is_measured_over_the_history_of_the_aggregated_entity() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!("aggregation-the-bucket-origin-is-measured-over-the-history-of-the-aggregated-entity", statement);
    }

    /// Clause 4.5.7: a deletion carries no value, so the rows that record one are
    /// outside every aggregate rather than counted as observations.
    #[test]
    fn tombstone_rows_are_excluded_from_every_aggregate() {
        let statement = statement_of(&aggregated(vec![AggrMethod::TotalCount], None));
        assert_statement_snapshot!("aggregation-tombstone-rows-are-excluded-from-every-aggregate", statement);
    }

    /// Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
    /// period. A fourth group column would answer one period with one entry per
    /// value the rows happened to disagree on, so the key holds those three and
    /// no others: each stream, an attribute and a `datasetId`, is aggregated on
    /// its own, and its rows are grouped by the period alone.
    #[test]
    fn the_group_key_is_the_attribute_the_dataset_and_the_period_alone() {
        let periodic = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!("aggregation-the-group-key-is-the-attribute-the-dataset-and-the-period-alone-1", periodic);

        // A stream with no instance has no period.
        let whole_range = statement_of(&aggregated(vec![AggrMethod::Avg], None));
        assert_statement_snapshot!("aggregation-the-group-key-is-the-attribute-the-dataset-and-the-period-alone-2", whole_range);
    }

    /// Keying the group on the unit, the attribute family or the value shape
    /// would silently split a period whenever one of them flipped inside it, so
    /// none of them may appear in the key.
    #[test]
    fn no_unit_or_registry_code_keys_the_group() {
        let methods = || vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::TotalCount];
        let periodic = statement_of(&aggregated(methods(), Some("PT1H")));
        assert_statement_snapshot!("aggregation-no-unit-or-registry-code-keys-the-group-1", periodic);
        let whole_range = statement_of(&aggregated(methods(), None));
        assert_statement_snapshot!("aggregation-no-unit-or-registry-code-keys-the-group-2", whole_range);
    }

    /// The family is a fact of the row rather than of the mutable catalogue, so
    /// it is read from the hypertable - as the span of the group's rows, since it
    /// does not key them. The value shape is spanned at the grain Tables
    /// 4.5.19.1-1 and 4.5.19.1-2 distinguish, which the stored `value_kind`
    /// code alone is too coarse to give.
    #[test]
    fn the_attribute_family_and_value_shape_are_projected_as_the_span_of_the_group() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!(
            "aggregation-the-attribute-family-and-value-shape-are-projected-as-the-span-of-the-group",
            statement
        );
    }

    /// Table 4.5.19.1-1, "JSON Array" column: `avg`, `min`, `max` and `sum` are
    /// taken over the sizes of the arrays inside the period, so each of them
    /// measures the JSONB column by its element count.
    #[test]
    fn the_size_measuring_methods_aggregate_an_array_by_its_length() {
        let avg = statement_of(&aggregated(vec![AggrMethod::Avg], None));
        assert_statement_snapshot!("aggregation-the-size-measuring-methods-aggregate-an-array-by-its-length-1", avg);
        let min = statement_of(&aggregated(vec![AggrMethod::Min], None));
        assert_statement_snapshot!("aggregation-the-size-measuring-methods-aggregate-an-array-by-its-length-2", min);
        let max = statement_of(&aggregated(vec![AggrMethod::Max], None));
        assert_statement_snapshot!("aggregation-the-size-measuring-methods-aggregate-an-array-by-its-length-3", max);
        let sum = statement_of(&aggregated(vec![AggrMethod::Sum], None));
        assert_statement_snapshot!("aggregation-the-size-measuring-methods-aggregate-an-array-by-its-length-4", sum);
    }

    /// Table 4.5.19.1-1 marks `stddev` and `sumsq` N/A in the "JSON Array"
    /// column while defining them for numbers and booleans, so neither may fold
    /// an array size in and answer a request the table refuses.
    #[test]
    fn standard_deviation_and_the_sum_of_squares_never_measure_an_array() {
        let std_dev = statement_of(&aggregated(vec![AggrMethod::StdDev], None));
        assert_statement_snapshot!("aggregation-standard-deviation-and-the-sum-of-squares-never-measure-an-array-1", std_dev);
        let sum_sq = statement_of(&aggregated(vec![AggrMethod::SumSq], None));
        assert_statement_snapshot!("aggregation-standard-deviation-and-the-sum-of-squares-never-measure-an-array-2", sum_sq);
    }

    /// Clause 4.5.2 attaches a `unitCode` to the instance, not to the attribute,
    /// so two instances of one attribute may differ in unit. `MIN` and `MAX` skip
    /// nulls, so the bounds alone cannot separate an all-annotated group from one
    /// that also held unannotated rows, and the null test is projected beside them.
    #[test]
    fn the_unit_span_carries_both_bounds_and_whether_any_row_carried_no_unit() {
        let periodic = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!("aggregation-the-unit-span-carries-both-bounds-and-whether-any-row-carried-no-unit-1", periodic);
        let whole_range = statement_of(&aggregated(vec![AggrMethod::Avg], None));
        assert_statement_snapshot!(
            "aggregation-the-unit-span-carries-both-bounds-and-whether-any-row-carried-no-unit-2",
            whole_range
        );
    }

    /// An exact decimal stays exact: the aggregate reads the `NUMERIC` value
    /// column rather than widening it to binary floating point.
    #[test]
    fn numeric_values_are_aggregated_without_a_float_cast() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Min, AggrMethod::Max, AggrMethod::Avg], None));
        assert_statement_snapshot!("aggregation-numeric-values-are-aggregated-without-a-float-cast", statement);
    }

    /// Clause 4.5.19: an empty or zero period aggregates the whole range into one
    /// bucket, which is what an absent period means, so no `time_bucket` is
    /// emitted for any of them. An unbounded query sets neither bound, so each
    /// side of its period is the extreme instant the stream holds.
    #[test]
    fn an_aggregation_period_of_zero_covers_the_whole_range() {
        let empty = statement_of(&aggregated(vec![AggrMethod::TotalCount], Some("")));
        assert_statement_snapshot!("aggregation-an-aggregation-period-of-zero-covers-the-whole-range-1", empty);
        let zero_time = statement_of(&aggregated(vec![AggrMethod::TotalCount], Some("PT0S")));
        assert_statement_snapshot!("aggregation-an-aggregation-period-of-zero-covers-the-whole-range-2", zero_time);
        let zero_days = statement_of(&aggregated(vec![AggrMethod::TotalCount], Some("P0D")));
        assert_statement_snapshot!("aggregation-an-aggregation-period-of-zero-covers-the-whole-range-3", zero_days);
    }

    /// Clause 4.5.19.1: the one period of a whole-range aggregation spans the
    /// time range of the temporal query, so a `between` query reports its own
    /// bounds as the period's start and end instants.
    #[test]
    fn a_whole_range_period_over_a_between_query_spans_the_query_bounds() {
        let from = "2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let to = "2024-01-01T03:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let mut compiled = aggregated(vec![AggrMethod::Avg], Some("PT0S"));
        compiled.window.interval = TimeInterval::Between { from, to };
        let statement = statement_of(&compiled);
        assert_statement_snapshot!("aggregation-a-whole-range-period-over-a-between-query-spans-the-query-bounds", statement);
    }

    /// A `before` query opens without limit, so its one period ends at
    /// `timeAt` and starts at the least recent instant the stream holds, which
    /// the response then widens to its own (clause 4.5.19.1).
    #[test]
    fn a_whole_range_period_over_a_before_query_opens_at_the_least_recent_instant_its_stream_holds() {
        let to = "2024-01-01T03:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let mut compiled = aggregated(vec![AggrMethod::Avg], None);
        compiled.window.interval = TimeInterval::Before(to);
        let statement = statement_of(&compiled);
        assert_statement_snapshot!(
            "aggregation-a-whole-range-period-over-a-before-query-opens-at-the-least-recent-instant-its-stream-holds",
            statement
        );
    }

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    /// The aggregation of `period` between 2026-03-15 and 2026-06-15.
    fn from_the_fifteenth(period: &str) -> CompiledTemporalQuery {
        let mut compiled = aggregated(vec![AggrMethod::TotalCount], Some(period));
        compiled.window.interval = TimeInterval::Between {
            from: instant("2026-03-15T00:00:00Z"),
            to: instant("2026-06-15T00:00:00Z"),
        };
        compiled
    }

    /// Clause 4.5.19.1: a duration of days and time keeps `time_bucket`, anchored
    /// at `timeAt`, its end the start plus the duration.
    #[test]
    fn an_exact_period_buckets_with_time_bucket_at_the_origin() {
        let hour = statement_of(&from_the_fifteenth("PT1H"));
        assert_statement_snapshot!("aggregation-an-exact-period-buckets-with-time-bucket-at-the-origin-1", hour);
        let day = statement_of(&from_the_fifteenth("P1D"));
        assert_statement_snapshot!("aggregation-an-exact-period-buckets-with-time-bucket-at-the-origin-2", day);
        let hour_and_a_half = statement_of(&from_the_fifteenth("PT1H30M"));
        assert_statement_snapshot!("aggregation-an-exact-period-buckets-with-time-bucket-at-the-origin-3", hour_and_a_half);
    }

    /// Clause 4.5.19.1 admits a duration with a year or month part, mixed with
    /// days or time too. `time_bucket` would keep only the year and month of
    /// `timeAt` and refuse the mixed ones, so period k is counted from `timeAt`
    /// in UTC instead, the period written into the statement once.
    #[test]
    fn a_calendar_period_numbers_its_periods_from_the_origin_in_utc() {
        let month = statement_of(&from_the_fifteenth("P1M"));
        assert_statement_snapshot!("aggregation-a-calendar-period-numbers-its-periods-from-the-origin-in-utc-1", month);
        let year = statement_of(&from_the_fifteenth("P1Y"));
        assert_statement_snapshot!("aggregation-a-calendar-period-numbers-its-periods-from-the-origin-in-utc-2", year);
        let month_and_day = statement_of(&from_the_fifteenth("P1M1D"));
        assert_statement_snapshot!("aggregation-a-calendar-period-numbers-its-periods-from-the-origin-in-utc-3", month_and_day);
        let month_and_hours = statement_of(&from_the_fifteenth("P1MT2H"));
        assert_statement_snapshot!("aggregation-a-calendar-period-numbers-its-periods-from-the-origin-in-utc-4", month_and_hours);
    }

    /// A month-end clamp makes a period end at the next period's start rather
    /// than at its own start plus the duration (from 31 January: 28 February to
    /// 31 March, where 28 February plus a month is 28 March), so the end is
    /// projected beside the start and keys the group with it.
    #[test]
    fn a_calendar_period_ends_at_the_next_periods_start() {
        let statement = statement_of(&from_the_fifteenth("P1M"));
        assert_statement_snapshot!("aggregation-a-calendar-period-ends-at-the-next-periods-start", statement);
    }

    /// A continuation opens on a boundary that lays its grid, so the grid is
    /// laid from the instant the window opens on, and every row reports it.
    #[test]
    fn a_continuation_lays_its_grid_from_the_instant_its_window_opens_on() {
        let mut compiled = from_the_fifteenth("P1M");
        compiled.window.interval = TimeInterval::After(instant("2026-05-15T00:00:00Z"));
        let statement = statement_of(&compiled);
        assert_statement_snapshot!("aggregation-a-continuation-lays-its-grid-from-the-instant-its-window-opens-on", statement);
    }

    /// The origin the periods were laid from is reported beside them, read
    /// from the history when the window names none.
    #[test]
    fn every_row_reports_the_origin_of_its_grid() {
        let named = statement_of(&from_the_fifteenth("PT1H"));
        assert_statement_snapshot!("aggregation-every-row-reports-the-origin-of-its-grid-1", named);
        let read = statement_of(&aggregated(vec![AggrMethod::TotalCount], Some("P1M")));
        assert_statement_snapshot!("aggregation-every-row-reports-the-origin-of-its-grid-2", read);
        let whole_range = statement_of(&aggregated(vec![AggrMethod::TotalCount], None));
        assert_statement_snapshot!("aggregation-every-row-reports-the-origin-of-its-grid-3", whole_range);
    }

    /// A retrieval without `timeAt` anchors a calendar grid where it anchors an
    /// exact one: at the earliest instant of the Entity's history on the axis.
    #[test]
    fn a_calendar_period_without_time_at_numbers_from_the_entitys_earliest_instant() {
        let statement = statement_of(&aggregated(vec![AggrMethod::TotalCount], Some("P1M")));
        assert_statement_snapshot!(
            "aggregation-a-calendar-period-without-time-at-numbers-from-the-entitys-earliest-instant",
            statement
        );
    }

    #[test]
    fn a_statement_not_asking_for_total_count_still_counts_each_period() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!("aggregation-a-statement-not-asking-for-total-count-still-counts-each-period", statement);
    }

    #[test]
    fn a_statement_asking_for_total_count_counts_each_period_once() {
        let statement = statement_of(&aggregated(vec![AggrMethod::TotalCount], Some("PT1H")));
        assert_statement_snapshot!("aggregation-a-statement-asking-for-total-count-counts-each-period-once", statement);
    }

    #[test]
    fn an_empty_method_list_still_counts() {
        let statement = statement_of(&aggregated(Vec::new(), None));
        assert_statement_snapshot!("aggregation-an-empty-method-list-still-counts", statement);
    }

    /// The renderer emits one triple per row, so the rows of one attribute and
    /// dataset have to arrive in period order rather than in whatever order the
    /// plan produced them: each stream numbers its periods, and the statement
    /// orders by stream and number.
    #[test]
    fn the_rows_arrive_ordered_by_attribute_dataset_and_period() {
        let aggregation = statement_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert_statement_snapshot!("aggregation-the-rows-arrive-ordered-by-attribute-dataset-and-period-1", aggregation);
        let read = build_temporal_history_query(
            &HistoryBranch::live(),
            slice::from_ref(&"urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("a valid identifier")),
            &aggregated(vec![AggrMethod::Avg], Some("PT1H")),
            &QueryNames::default(),
        )
        .expect("a renderable history read");
        assert_statement_snapshot!("aggregation-the-rows-arrive-ordered-by-attribute-dataset-and-period-2", read);
    }

    /// `aggrGroupBy=unitCode` promotes the unit to a group key: `ah.unit_code_id`
    /// joins the `GROUP BY`, while the span projection that decides a mixed group
    /// is unchanged.
    #[test]
    fn grouping_by_unit_code_adds_the_unit_column_to_the_group_by() {
        let statement = statement_of(&grouped(vec![AggrMethod::Avg], None, "unitCode"));
        assert_statement_snapshot!("aggregation-grouping-by-unit-code-adds-the-unit-column-to-the-group-by", statement);
    }

    /// `aggrGroupBy=observedAt;month` truncates the observation axis to a calendar
    /// month, the calendar bucketing clause 4.5.19's fixed-width period cannot
    /// express.
    #[test]
    fn a_calendar_bucket_truncates_the_axis_in_the_group_by() {
        let statement = statement_of(&grouped(vec![AggrMethod::Avg], None, "observedAt;month"));
        assert_statement_snapshot!("aggregation-a-calendar-bucket-truncates-the-axis-in-the-group-by", statement);
    }

    /// `aggrGroupBy=observedAt;hourOfDay` folds the axis onto the hour of day,
    /// the non-contiguous cyclic bucketing.
    #[test]
    fn a_cyclic_bucket_extracts_the_axis_field_in_the_group_by() {
        let statement = statement_of(&grouped(vec![AggrMethod::Avg], None, "observedAt;hourOfDay"));
        assert_statement_snapshot!("aggregation-a-cyclic-bucket-extracts-the-axis-field-in-the-group-by", statement);
    }

    /// A value-in-effect key correlates the attribute's value at each instant
    /// through a lateral join selecting the latest instance not after the row.
    #[test]
    fn a_value_key_joins_the_value_in_effect_through_a_lateral() {
        let statement = statement_of(&grouped(vec![AggrMethod::Avg], None, "category"));
        assert_statement_snapshot!("aggregation-a-value-key-joins-the-value-in-effect-through-a-lateral", statement);
    }

    /// Grouping combines with `aggrPeriodDuration`: the period column stays, and
    /// the group key sits before it in the `GROUP BY`.
    #[test]
    fn grouping_combines_with_the_aggregation_period() {
        let statement = statement_of(&grouped(vec![AggrMethod::Avg], Some("PT1H"), "unitCode"));
        assert_statement_snapshot!("aggregation-grouping-combines-with-the-aggregation-period", statement);
    }

    #[test]
    fn min_and_max_project_a_typed_column_per_kind() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Min, AggrMethod::Max], None));
        assert_statement_snapshot!("aggregation-min-and-max-project-a-typed-column-per-kind", statement);
    }

    /// Clause 5.7.4: an aggregation on the live branch reads the entity's
    /// queued entries beside its recorded rows, so a written observation is
    /// aggregated before the drain applies it.
    #[test]
    fn aggregation_reads_the_pending_rows() {
        let statement = statement_of(&aggregated(vec![AggrMethod::Sum, AggrMethod::Max], None));
        assert_statement_snapshot!("aggregation-aggregation-reads-the-pending-rows", statement);
    }
}
