//! Aggregated temporal values: `time_bucket` windows and the aggregate
//! expressions of clause 4.5.19.
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

use crate::{
    aggregated_value::{arithmetic_numeric_expr, average_time_expr, distinct_value_expr, ordered_instant_expr, ordered_text_expr, sized_numeric_expr},
    aggregation_grouping::apply_grouping,
    alias::{Ah, An},
    axis::axis_column,
    history::base_history_source,
    recorded_shape::recorded_shape,
};
use andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::history_source::{HistoryBranch, HistoryOf, attribute_rows::attribute_rows, grain::HistoryGrain};
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, temporal::AttributeHistory};
use andromeda_postgresql_seaquery::{
    aggregate::{bool_or, count_all, count_distinct, stddev},
    cast::{cast, typed_null},
    literal::int_literal,
    pg_type::PostgresType,
    timescale::time_bucket,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Condition, Expr, ExprTrait, Func, Iden, JoinType, Order, Query, SelectStatement, SimpleExpr};

/// Lateral projection that computes one bucket boundary per history row.
#[derive(Iden, Clone, Copy)]
#[iden = "bucket"]
struct Bucket;

/// Start instant projected by [`Bucket`].
#[derive(Iden, Clone, Copy)]
#[iden = "bucket_start"]
struct BucketStart;

/// Lateral projection of the implicit per-entity bucket origin.
#[derive(Iden, Clone, Copy)]
#[iden = "bucket_origin"]
struct BucketOrigin;

/// Origin instant projected by [`BucketOrigin`].
#[derive(Iden, Clone, Copy)]
#[iden = "origin"]
struct Origin;

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
fn aggregate_columns(methods: &[AggrMethod]) -> Vec<(SimpleExpr, &'static str)> {
    let mut columns: Vec<(SimpleExpr, &'static str)> = Vec::new();
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
            AggrMethod::TotalCount => columns.push((count_all(), "agg_total_count")),
            AggrMethod::DistinctCount => columns.push((count_distinct(distinct_value_expr()), "agg_distinct_count")),
        }
    }
    if columns.is_empty() {
        columns.push((count_all(), "agg_total_count"));
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

/// Project a history row's bucket start exactly once.
///
/// The outer aggregation groups on the projected column instead of repeating
/// the expression over `ah.<axis>` in its select list. PostgreSQL can therefore
/// validate the grouped statement without treating the time-axis column as an
/// ungrouped outer projection.
fn bucket_subquery(axis: AttributeHistory, width: SimpleExpr, origin: SimpleExpr) -> SelectStatement {
    Query::select()
        .expr_as(time_bucket(width, Expr::col((Ah, axis)), origin), BucketStart)
        .to_owned()
}

/// One bound of the whole-range period, typed so the column decodes the same
/// whether the query set that bound or left it open.
fn whole_range_bound(instant: Option<&DateTime<Utc>>) -> SimpleExpr {
    match instant {
        Some(instant) => cast(Expr::val(*instant), PostgresType::Timestamptz),
        None => typed_null(PostgresType::Timestamptz),
    }
}

/// Build the aggregation query with `time_bucket` windows over the rows `of`
/// reads.
#[must_use]
pub fn build_aggregation_query(branch: &HistoryBranch, of: &HistoryOf, compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
    // Clause 4.5.19: an absent period aggregates the whole range into one
    // bucket rather than bucketing at all. A zero period is absent by
    // construction, so there is nothing to test for here.
    let period = compiled.representation.aggr_period_duration;
    // Bucket on the same time axis the WHERE clause filters on, which for the
    // default selector of clause 4.11 is `observed_at` alone. Bucketing on the
    // operational write clock would assign a backfilled observation to a
    // different period than the one its window filter selected.
    let axis = axis_column(compiled.window.timeproperty);

    let mut select = Query::select();
    select.expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"));
    select.expr(Expr::col((Ah, HistoryGrain::DatasetId)));
    project_group_span(&mut select);
    base_history_source(&mut select, branch, of);

    if let Some(period) = period {
        let iso = period.as_iso8601();
        // The reference instant of clause 4.5.19 aligns the buckets; absent,
        // they align to the actual data range (not epoch).
        let origin = if let Some(reference) = compiled.window.interval.reference() {
            Expr::val(*reference)
        } else {
            select.join_lateral(JoinType::Join, origin_subquery(branch, of, axis), BucketOrigin, SimpleExpr::from(true));
            Expr::col((BucketOrigin, Origin))
        };
        select.join_lateral(
            JoinType::Join,
            bucket_subquery(axis, cast(Expr::val(iso.clone()), PostgresType::Interval), origin),
            Bucket,
            SimpleExpr::from(true),
        );
        let bucket_start = Expr::col((Bucket, BucketStart));
        let bucket_end = bucket_start.clone().add(cast(Expr::val(iso), PostgresType::Interval));
        select.expr_as(bucket_start, BucketStart);
        select.expr_as(bucket_end, Alias::new("bucket_end"));
    } else {
        // Clause 4.5.19.1: the one period spans the whole time range of the
        // temporal query, so its start and end DateTimes are the query bounds.
        // A side the query leaves open names no instant.
        let interval = compiled.window.interval;
        select.expr_as(whole_range_bound(interval.opening()), BucketStart);
        select.expr_as(whole_range_bound(interval.closing()), Alias::new("bucket_end"));
    }

    for (expr, alias) in aggregate_columns(&compiled.representation.aggr_methods) {
        select.expr_as(expr, Alias::new(alias));
    }

    // Clause 4.5.7: a tombstone records a deletion and carries no value, so the
    // rows that record one are outside every aggregate rather than counted.
    select.cond_where(condition.add(Expr::col((Ah, AttributeHistory::ValueKind)).ne(int_literal(0))));

    // Clause 4.5.19: the fixed key is the attribute and `datasetId`. The
    // `aggrGroupBy` extension inserts its own keys between that fixed key and
    // the period, and an absent period is the whole range: one bucket rather
    // than no grouping at all.
    select.add_group_by([
        SimpleExpr::from(Expr::col((An, AttributeNames::Name))),
        SimpleExpr::from(Expr::col((Ah, HistoryGrain::DatasetId))),
    ]);
    select
        .order_by((An, AttributeNames::Name), Order::Asc)
        .order_by((Ah, HistoryGrain::DatasetId), Order::Asc);

    if let Some(grouping) = &compiled.representation.grouping {
        apply_grouping(&mut select, grouping, axis, branch, of);
    }

    if period.is_some() {
        select.add_group_by([SimpleExpr::from(Expr::col((Bucket, BucketStart)))]);
        select.order_by((Bucket, BucketStart), Order::Asc);
    }

    select.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{
            aggregation_period::AggregationPeriod,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::history_source::{HistoryBranch, HistoryOf};
    use sea_query::PostgresQueryBuilder;

    fn aggregated(methods: Vec<AggrMethod>, period: Option<&str>) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::AggregatedValues)
                    .aggr_methods(methods)
                    .aggr_period_duration(period.and_then(|raw| AggregationPeriod::parse(raw).expect("valid period")))
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn grouped(methods: Vec<AggrMethod>, period: Option<&str>, aggr_group_by: &str) -> CompiledTemporalQuery {
        use andromeda_ngsi_ld_query::temporal::grouping::AggregationGrouping;
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::GroupedValues)
                    .aggr_methods(methods)
                    .aggr_period_duration(period.and_then(|raw| AggregationPeriod::parse(raw).expect("valid period")))
                    .grouping(Some(AggregationGrouping::parse(aggr_group_by).expect("a well-formed grouping")))
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    /// The entity the aggregation reads the history of.
    fn vehicle() -> HistoryOf {
        HistoryOf::Entity(Expr::val("urn:ngsi-ld:Vehicle:1"))
    }

    fn sql_of(compiled: &CompiledTemporalQuery) -> String {
        build_aggregation_query(&HistoryBranch::live(), &vehicle(), compiled, Condition::all()).to_string(PostgresQueryBuilder)
    }

    fn bound_sql_of(compiled: &CompiledTemporalQuery) -> (String, sea_query::Values) {
        build_aggregation_query(&HistoryBranch::live(), &vehicle(), compiled, Condition::all()).build(PostgresQueryBuilder)
    }

    #[test]
    fn a_period_produces_a_bucketed_group_by() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert!(sql.contains("time_bucket"), "{sql}");
        assert!(sql.contains("bucket_start"), "{sql}");
    }

    #[test]
    fn the_bound_statement_projects_one_bucket_expression_and_reuses_its_result() {
        let (sql, _values) = bound_sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert!(sql.contains("JOIN LATERAL (SELECT time_bucket("), "{sql}");
        assert!(sql.contains("AS \"bucket_start\") AS \"bucket\""), "{sql}");
        assert!(sql.contains("\"bucket\".\"bucket_start\" +"), "{sql}");
        pretty_assertions::assert_eq!(sql.matches("\"ah\".\"observed_at\"").count(), 1, "{sql}");
        // The bound width, not every interval cast in the statement: an `avg`
        // also projects the Table 4.5.19.1-2 mean time of day, which casts a
        // time column to an interval for reasons unrelated to bucketing. A
        // bucket width is a bound parameter, so it is the cast that follows a
        // placeholder.
        let bound_widths = sql
            .match_indices("::interval")
            .filter(|(index, _)| sql[..*index].ends_with(|character: char| character.is_ascii_digit()))
            .count();
        pretty_assertions::assert_eq!(bound_widths, 2, "the bucket width and bucket-end width must each be bound once: {sql}");
    }

    /// Clause 4.5.19: the bucket is computed on the same axis the window filters
    /// on, and clause 4.11 makes that `observedAt` by default, so a backfilled
    /// observation lands in the period its filter selected. The correlated origin
    /// subquery measures the same axis under its own alias.
    #[test]
    fn observed_at_buckets_on_the_observation_axis_not_write_time() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert!(sql.contains("time_bucket('PT1H'::interval, \"ah\".\"observed_at\""), "{sql}");
        assert!(sql.contains("MIN(\"ah2\".\"observed_at\")"), "{sql}");
        assert!(!sql.contains("\"ah\".\"recorded_at\""), "{sql}");
        pretty_assertions::assert_eq!(sql.matches("SELECT MIN(").count(), 1, "{sql}");
    }

    /// The bucket origin is measured over the same entity's streams, reached
    /// through the stream catalogue rather than through a current-state row.
    #[test]
    fn the_bucket_origin_is_measured_over_the_history_of_the_aggregated_entity() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        let origin = sql.split("AS \"origin\" FROM").nth(1).expect("the origin subquery");
        let origin = origin.split("AS \"ah2\"").next().expect("the origin source");
        assert!(origin.contains("\"re\".\"id\" = 'urn:ngsi-ld:Vehicle:1'"), "{sql}");
        assert!(origin.contains("\"pe\".\"entity_id\" = 'urn:ngsi-ld:Vehicle:1'"), "{sql}");
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
    }

    /// Clause 4.5.7: a deletion carries no value, so the rows that record one are
    /// outside every aggregate rather than counted as observations.
    #[test]
    fn tombstone_rows_are_excluded_from_every_aggregate() {
        let sql = sql_of(&aggregated(vec![AggrMethod::TotalCount], None));
        assert!(sql.contains("\"ah\".\"value_kind\" <> (0)"), "{sql}");
    }

    /// Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
    /// period. A fourth group column would answer one period with one entry per
    /// value the rows happened to disagree on, so the key holds those three and
    /// no others.
    #[test]
    fn the_group_key_is_the_attribute_the_dataset_and_the_period_alone() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        let group_by = sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        let group_by = group_by.split("ORDER BY").next().expect("a clause before ORDER BY").trim();
        pretty_assertions::assert_eq!(group_by, "\"an\".\"name\", \"ah\".\"dataset_id\", \"bucket\".\"bucket_start\"", "{sql}");

        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], None));
        let group_by = sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        let group_by = group_by.split("ORDER BY").next().expect("a clause before ORDER BY").trim();
        pretty_assertions::assert_eq!(group_by, "\"an\".\"name\", \"ah\".\"dataset_id\"", "{sql}");
    }

    /// Keying the group on the unit, the attribute family or the value shape
    /// would silently split a period whenever one of them flipped inside it, so
    /// none of them may appear in the key.
    #[test]
    fn no_unit_or_registry_code_keys_the_group() {
        for period in [Some("PT1H"), None] {
            let sql = sql_of(&aggregated(vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::TotalCount], period));
            let group_by = sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
            for column in ["unit_code_id", "value_kind", "attribute_kind"] {
                assert!(!group_by.contains(column), "{column} still keys the group: {sql}");
            }
        }
    }

    /// The family is a fact of the row rather than of the mutable catalogue, so
    /// it is read from the hypertable - as the span of the group's rows, since it
    /// does not key them. The value shape is spanned at the grain Tables
    /// 4.5.19.1-1 and 4.5.19.1-2 distinguish, which the stored `value_kind`
    /// code alone is too coarse to give.
    #[test]
    fn the_attribute_family_and_value_shape_are_projected_as_the_span_of_the_group() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        for projection in [
            "MIN(\"ah\".\"attribute_kind\") AS \"attribute_kind_lowest\"",
            "MAX(\"ah\".\"attribute_kind\") AS \"attribute_kind_highest\"",
            "AS \"recorded_shape_lowest\"",
            "AS \"recorded_shape_highest\"",
        ] {
            assert!(sql.contains(projection), "{projection} missing: {sql}");
        }
        assert!(sql.contains("jsonb_typeof"), "the shape span separates an array from an object: {sql}");
        assert!(!sql.contains("attribute_type"), "{sql}");
        assert!(!sql.contains("\"ah\".\"value_type"), "{sql}");
    }

    /// Table 4.5.19.1-1, "JSON Array" column: `avg`, `min`, `max` and `sum` are
    /// taken over the sizes of the arrays inside the period, so each of them
    /// measures the JSONB column by its element count.
    #[test]
    fn the_size_measuring_methods_aggregate_an_array_by_its_length() {
        for method in [AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max, AggrMethod::Sum] {
            let sql = sql_of(&aggregated(vec![method], None));
            assert!(
                sql.contains("jsonb_array_length(COALESCE((CASE WHEN ((\"ah\".\"value_object\" ->> ('@type')) = ('@json'))"),
                "{method}: {sql}"
            );
        }
    }

    /// Table 4.5.19.1-1 marks `stddev` and `sumsq` N/A in the "JSON Array"
    /// column while defining them for numbers and booleans, so neither may fold
    /// an array size in and answer a request the table refuses.
    #[test]
    fn standard_deviation_and_the_sum_of_squares_never_measure_an_array() {
        for method in [AggrMethod::StdDev, AggrMethod::SumSq] {
            let sql = sql_of(&aggregated(vec![method], None));
            assert!(!sql.contains("jsonb_array_length"), "{method}: {sql}");
            assert!(sql.contains("\"ah\".\"value_boolean\"::integer::numeric"), "{method}: {sql}");
        }
    }

    /// Clause 4.5.2 attaches a `unitCode` to the instance, not to the attribute,
    /// so two instances of one attribute may differ in unit. `MIN` and `MAX` skip
    /// nulls, so the bounds alone cannot separate an all-annotated group from one
    /// that also held unannotated rows, and the null test is projected beside them.
    #[test]
    fn the_unit_span_carries_both_bounds_and_whether_any_row_carried_no_unit() {
        for period in [Some("PT1H"), None] {
            let sql = sql_of(&aggregated(vec![AggrMethod::Avg], period));
            for projection in [
                "MIN(\"ah\".\"unit_code_id\") AS \"unit_code_id_lowest\"",
                "MAX(\"ah\".\"unit_code_id\") AS \"unit_code_id_highest\"",
                "BOOL_OR(\"ah\".\"unit_code_id\" IS NULL) AS \"any_unitless\"",
            ] {
                assert!(sql.contains(projection), "{projection} missing: {sql}");
            }
        }
    }

    /// An exact decimal stays exact: the aggregate reads the `NUMERIC` value
    /// column rather than widening it to binary floating point.
    #[test]
    fn numeric_values_are_aggregated_without_a_float_cast() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Min, AggrMethod::Max, AggrMethod::Avg], None));
        assert!(!sql.contains("float8"), "{sql}");
        assert!(sql.contains("\"ah\".\"value_boolean\"::integer::numeric"), "{sql}");
    }

    /// Clause 4.5.19: an empty or zero period aggregates the whole range into one
    /// bucket, which is what an absent period means, so no `time_bucket` is
    /// emitted for any of them.
    #[test]
    fn an_aggregation_period_of_zero_covers_the_whole_range() {
        for period in ["", "PT0S", "P0D"] {
            let sql = sql_of(&aggregated(vec![AggrMethod::TotalCount], Some(period)));
            assert!(!sql.contains("time_bucket"), "{period}: {sql}");
            assert!(sql.contains("NULL::timestamptz AS \"bucket_start\""), "{period}: {sql}");
        }
    }

    /// Clause 4.5.19.1: the one period of a whole-range aggregation spans the
    /// time range of the temporal query, so a `between` query reports its own
    /// bounds as the period's start and end instants.
    #[test]
    fn a_whole_range_period_over_a_between_query_spans_the_query_bounds() {
        let from = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").expect("valid instant").with_timezone(&Utc);
        let to = DateTime::parse_from_rfc3339("2024-01-01T03:00:00Z").expect("valid instant").with_timezone(&Utc);
        let mut compiled = aggregated(vec![AggrMethod::Avg], Some("PT0S"));
        compiled.window.interval = TimeInterval::Between { from, to };
        let sql = sql_of(&compiled);
        assert!(!sql.contains("time_bucket"), "{sql}");
        assert!(sql.contains("'2024-01-01 00:00:00.000000 +00:00'::timestamptz AS \"bucket_start\""), "{sql}");
        assert!(sql.contains("'2024-01-01 03:00:00.000000 +00:00'::timestamptz AS \"bucket_end\""), "{sql}");
    }

    /// A `before` query opens without limit, so only the end of its one period
    /// is an instant.
    #[test]
    fn a_whole_range_period_over_a_before_query_names_only_its_closing_instant() {
        let to = DateTime::parse_from_rfc3339("2024-01-01T03:00:00Z").expect("valid instant").with_timezone(&Utc);
        let mut compiled = aggregated(vec![AggrMethod::Avg], None);
        compiled.window.interval = TimeInterval::Before(to);
        let sql = sql_of(&compiled);
        assert!(sql.contains("NULL::timestamptz AS \"bucket_start\""), "{sql}");
        assert!(sql.contains("'2024-01-01 03:00:00.000000 +00:00'::timestamptz AS \"bucket_end\""), "{sql}");
    }

    #[test]
    fn an_empty_method_list_still_counts() {
        let sql = sql_of(&aggregated(Vec::new(), None));
        assert!(sql.contains("agg_total_count"), "{sql}");
    }

    /// The renderer emits one triple per row, so the rows of one attribute and
    /// dataset have to arrive in period order rather than in whatever order the
    /// plan produced them.
    #[test]
    fn the_rows_arrive_ordered_by_attribute_dataset_and_period() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        let order_by = sql.split("ORDER BY").nth(1).expect("an ORDER BY clause").trim();
        pretty_assertions::assert_eq!(
            order_by,
            "\"an\".\"name\" ASC, \"ah\".\"dataset_id\" ASC, \"bucket\".\"bucket_start\" ASC",
            "{sql}"
        );
    }

    /// `aggrGroupBy=unitCode` promotes the unit to a group key: `ah.unit_code_id`
    /// joins the `GROUP BY`, while the span projection that decides a mixed group
    /// is unchanged.
    #[test]
    fn grouping_by_unit_code_adds_the_unit_column_to_the_group_by() {
        let sql = sql_of(&grouped(vec![AggrMethod::Avg], None, "unitCode"));
        let group_by = sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        let group_by = group_by.split("ORDER BY").next().expect("a clause before ORDER BY");
        assert!(group_by.contains("\"ah\".\"unit_code_id\""), "unit_code_id must key the group: {sql}");
        // The span projection survives so the reader still reads the group's unit.
        assert!(sql.contains("AS \"unit_code_id_lowest\""), "{sql}");
    }

    /// `aggrGroupBy=observedAt;month` truncates the observation axis to a calendar
    /// month, the calendar bucketing clause 4.5.19's fixed-width period cannot
    /// express.
    #[test]
    fn a_calendar_bucket_truncates_the_axis_in_the_group_by() {
        let sql = sql_of(&grouped(vec![AggrMethod::Avg], None, "observedAt;month"));
        assert!(sql.contains("date_trunc('month', \"ah\".\"observed_at\")"), "{sql}");
        let group_by = sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        assert!(group_by.contains("date_trunc('month'"), "the bucket keys the group: {sql}");
    }

    /// `aggrGroupBy=observedAt;hourOfDay` folds the axis onto the hour of day,
    /// the non-contiguous cyclic bucketing.
    #[test]
    fn a_cyclic_bucket_extracts_the_axis_field_in_the_group_by() {
        let sql = sql_of(&grouped(vec![AggrMethod::Avg], None, "observedAt;hourOfDay"));
        assert!(sql.contains("date_part('hour', \"ah\".\"observed_at\")"), "{sql}");
    }

    /// A value-in-effect key correlates the attribute's value at each instant
    /// through a lateral join selecting the latest instance not after the row.
    #[test]
    fn a_value_key_joins_the_value_in_effect_through_a_lateral() {
        let sql = sql_of(&grouped(vec![AggrMethod::Avg], None, "category"));
        assert!(sql.contains("JOIN LATERAL"), "{sql}");
        assert!(sql.contains("ORDER BY \"ahk0\".\"observed_at\" DESC"), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    /// Grouping combines with `aggrPeriodDuration`: the period column stays, and
    /// the group key sits before it in the `GROUP BY`.
    #[test]
    fn grouping_combines_with_the_aggregation_period() {
        let sql = sql_of(&grouped(vec![AggrMethod::Avg], Some("PT1H"), "unitCode"));
        assert!(sql.contains("time_bucket"), "the period bucket survives grouping: {sql}");
        let group_by = sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        let group_by = group_by.split("ORDER BY").next().expect("a clause before ORDER BY");
        let unit_at = group_by.find("unit_code_id").expect("the unit keys the group");
        let period_at = group_by.find("bucket_start").expect("the period keys the group");
        assert!(unit_at < period_at, "the group key must precede the period: {group_by}");
    }

    #[test]
    fn min_and_max_project_a_typed_column_per_kind() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Min, AggrMethod::Max], None));
        for column in ["agg_min", "agg_min_str", "agg_min_dt", "agg_max", "agg_max_str", "agg_max_dt"] {
            assert!(sql.contains(column), "{column} missing: {sql}");
        }
    }

    /// Clause 5.7.4: an aggregation on the live branch reads the entity's
    /// queued entries beside its recorded rows, so a written observation is
    /// aggregated before the drain applies it.
    #[test]
    fn aggregation_reads_the_pending_rows() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Sum, AggrMethod::Max], None));
        let source = sql.split(r#") AS "ah" JOIN "attribute_names""#).next().expect("the attribute source");
        assert!(source.contains(r#"UNION ALL (SELECT "pe"."entity_id" AS "entity_id""#), "{sql}");
        assert!(source.contains(r#""pe"."entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
    }
}
