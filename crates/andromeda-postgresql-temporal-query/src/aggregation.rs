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
//! is also a statement about a unit — clause 4.5.2 lets every Property instance
//! carry its own `unitCode`, and nothing in the specification says what the mean
//! of a kilogram and a pound is, while Andromeda never converts
//! (`docs/unit-handling.md`). The unit identity and the two registry codes are
//! therefore projected as their own extrema over the group, and
//! [`crate::aggregation_group`] decides from that span whether the group is
//! answerable at all.

use crate::{
    alias::{Ah, An, S},
    axis::axis_column,
    history::base_history_source,
};
use andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_schema::{
    attribute_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    aggregate::{bool_or, count_all, count_distinct, stddev},
    cast::{cast, typed_null},
    jsonb::{jsonb_array_length, jsonb_typeof},
    literal::int_literal,
    pg_type::PostgresType,
    timescale::time_bucket,
};
use sea_query::{Alias, CaseStatement, Condition, Expr, Func, JoinType, Order, Query, SelectStatement, SimpleExpr};

/// Unified numeric expression for type-aware aggregation (NGSI-LD 4.5.19.1).
///
/// Booleans count as 0/1 and arrays as their length, so `avg` and `sum` mean
/// something for attributes the client did not write as numbers.
///
/// Every branch is `NUMERIC`, the type the value column itself carries: a JSON
/// number is an exact decimal (clause 4.5.2 gives no precision limit), and
/// widening it to binary floating point to aggregate would report a minimum or a
/// maximum that was never observed.
fn numeric_expr() -> SimpleExpr {
    let bool_as_numeric = cast(
        cast(Expr::col((Ah, AttributeHistory::ValueBool)).into(), PostgresType::Integer),
        PostgresType::Numeric,
    );
    let array_length = CaseStatement::new().case(
        Expr::expr(jsonb_typeof(Expr::col((Ah, AttributeHistory::ValueObject)).into())).eq("array"),
        cast(jsonb_array_length(Expr::col((Ah, AttributeHistory::ValueObject)).into()), PostgresType::Numeric),
    );
    Func::coalesce([Expr::col((Ah, AttributeHistory::ValueNumber)).into(), bool_as_numeric, array_length.into()]).into()
}

/// The span of registry codes and unit identities one aggregation group covered.
///
/// Every entry is an aggregate over the group rather than a group key, which is
/// what keeps the group key at the attribute, dataset and period clause 4.5.19
/// names. The family and the value shape live on the row rather than in the
/// catalogue, so an attribute whose type changed within a period is one period
/// whose rows disagree, not two periods. `MIN`/`MAX` skip nulls, so the unit
/// span also carries whether any row carried no unit at all.
fn project_group_span(select: &mut SelectStatement) {
    select.expr_as(Func::min(Expr::col((Ah, AttributeHistory::AttributeKind))), Alias::new("attribute_kind_lowest"));
    select.expr_as(
        Func::max(Expr::col((Ah, AttributeHistory::AttributeKind))),
        Alias::new("attribute_kind_highest"),
    );
    select.expr_as(Func::min(Expr::col((Ah, AttributeHistory::ValueKind))), Alias::new("value_kind_lowest"));
    select.expr_as(Func::max(Expr::col((Ah, AttributeHistory::ValueKind))), Alias::new("value_kind_highest"));
    select.expr_as(Func::min(Expr::col((Ah, AttributeHistory::UnitCodeId))), Alias::new("unit_code_id_lowest"));
    select.expr_as(Func::max(Expr::col((Ah, AttributeHistory::UnitCodeId))), Alias::new("unit_code_id_highest"));
    select.expr_as(bool_or(Expr::col((Ah, AttributeHistory::UnitCodeId)).is_null()), Alias::new("any_unitless"));
}

/// The aggregate projections clause 4.5.19 asks for, one per method, plus the
/// typed columns `min`/`max` read from.
fn aggregate_columns(methods: &[AggrMethod]) -> Vec<(SimpleExpr, &'static str)> {
    let mut columns: Vec<(SimpleExpr, &'static str)> = Vec::new();
    for method in methods {
        match method {
            AggrMethod::Avg => columns.push((Func::avg(numeric_expr()).into(), "agg_avg")),
            AggrMethod::Min => {
                columns.push((Func::min(numeric_expr()).into(), "agg_min"));
                columns.push((Func::min(Expr::col((Ah, AttributeHistory::ValueString))).into(), "agg_min_str"));
                columns.push((Func::min(Expr::col((Ah, AttributeHistory::ValueDatetime))).into(), "agg_min_dt"));
            }
            AggrMethod::Max => {
                columns.push((Func::max(numeric_expr()).into(), "agg_max"));
                columns.push((Func::max(Expr::col((Ah, AttributeHistory::ValueString))).into(), "agg_max_str"));
                columns.push((Func::max(Expr::col((Ah, AttributeHistory::ValueDatetime))).into(), "agg_max_dt"));
            }
            AggrMethod::Sum => columns.push((Func::sum(numeric_expr()).into(), "agg_sum")),
            AggrMethod::SumSq => columns.push((Func::sum(numeric_expr().mul(numeric_expr())).into(), "agg_sumsq")),
            AggrMethod::StdDev => columns.push((stddev(numeric_expr()), "agg_stddev")),
            AggrMethod::TotalCount => columns.push((count_all(), "agg_total_count")),
            AggrMethod::DistinctCount => columns.push((count_distinct(distinct_value_expr()), "agg_distinct_count")),
        }
    }
    if columns.is_empty() {
        columns.push((count_all(), "agg_total_count"));
    }
    columns
}

/// The single text projection `distinctCount` counts unique values of, folding
/// every typed value column into one comparable text.
fn distinct_value_expr() -> SimpleExpr {
    Func::coalesce([
        cast(Expr::col((Ah, AttributeHistory::ValueNumber)).into(), PostgresType::Text),
        Expr::col((Ah, AttributeHistory::ValueString)).into(),
        cast(Expr::col((Ah, AttributeHistory::ValueBool)).into(), PostgresType::Text),
        cast(Expr::col((Ah, AttributeHistory::ValueDatetime)).into(), PostgresType::Text),
        Expr::col((Ah, AttributeHistory::ValueUri)).into(),
        cast(Expr::col((Ah, AttributeHistory::ValueObject)).into(), PostgresType::Text),
    ])
    .into()
}

/// The bucket origin, measured over the same entity's streams through the stream
/// catalogue rather than through a current-state row.
///
/// Derived once per selected entity and reused for both bucket bounds, rather
/// than evaluating the same aggregate expression once for each rendered column.
fn origin_subquery(axis: AttributeHistory) -> SelectStatement {
    let ah2 = Alias::new("ah2");
    let s2 = Alias::new("s2");
    let mut origin = Query::select();
    origin
        .expr_as(Func::min(Expr::col((ah2.clone(), axis))), Alias::new("origin"))
        .from_as(AttributeHistory::Table, ah2.clone())
        .join_as(
            JoinType::Join,
            TemporalAttributeStreams::Table,
            s2.clone(),
            Expr::col((s2.clone(), TemporalAttributeStreams::Id)).equals((ah2, AttributeHistory::StreamId)),
        )
        .and_where(Expr::col((s2, TemporalAttributeStreams::TemporalEntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)));
    origin.take()
}

/// Build the aggregation query with `time_bucket` windows.
pub fn build_aggregation_query(compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
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
    select.expr(Expr::col((S, TemporalAttributeStreams::DatasetId)));
    project_group_span(&mut select);

    let mut needs_origin_join = false;
    if let Some(period) = period {
        let iso = period.as_iso8601();
        let origin = match compiled.window.interval.reference() {
            // The reference instant of clause 4.5.19 aligns the buckets.
            Some(reference) => Expr::val(*reference).into(),
            // Absent, buckets align to the actual data range (not epoch).
            None => {
                needs_origin_join = true;
                Expr::col((Alias::new("bucket_origin"), Alias::new("origin"))).into()
            }
        };
        let bucket_start = time_bucket(
            cast(Expr::val(iso.clone()).into(), PostgresType::Interval),
            Expr::col((Ah, axis)).into(),
            origin,
        );
        let bucket_end = bucket_start.clone().add(cast(Expr::val(iso).into(), PostgresType::Interval));
        select.expr_as(bucket_start, Alias::new("bucket_start"));
        select.expr_as(bucket_end, Alias::new("bucket_end"));
    } else {
        select.expr_as(typed_null(PostgresType::Timestamptz), Alias::new("bucket_start"));
        select.expr_as(typed_null(PostgresType::Timestamptz), Alias::new("bucket_end"));
    }

    for (expr, alias) in aggregate_columns(&compiled.representation.aggr_methods) {
        select.expr_as(expr, Alias::new(alias));
    }

    base_history_source(&mut select);
    if needs_origin_join {
        select.join_lateral(JoinType::Join, origin_subquery(axis), Alias::new("bucket_origin"), SimpleExpr::from(true));
    }

    // Clause 4.5.7: a tombstone records a deletion and carries no value, so the
    // rows that record one are outside every aggregate rather than counted.
    select.cond_where(condition.add(Expr::col((Ah, AttributeHistory::ValueKind)).ne(int_literal(0))));

    // Clause 4.5.19: the group key is the attribute, `datasetId` and period, and
    // nothing else. An absent period is the whole range, which is one bucket
    // rather than no grouping at all.
    let mut group = vec![
        Expr::col((An, AttributeNames::Name)).into(),
        SimpleExpr::from(Expr::col((S, TemporalAttributeStreams::DatasetId))),
    ];
    if period.is_some() {
        group.push(Expr::col(Alias::new("bucket_start")).into());
    }
    select.add_group_by(group);

    select
        .order_by((An, AttributeNames::Name), Order::Asc)
        .order_by((S, TemporalAttributeStreams::DatasetId), Order::Asc);
    if period.is_some() {
        select.order_by(Alias::new("bucket_start"), Order::Asc);
    }

    select.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alias::Te;
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{aggregation_period::AggregationPeriod, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_schema::temporal::TemporalEntities;
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

    /// The entity predicate stage 2 threads into the aggregation, standing in
    /// for the `te.id = <uri>` the history builder computes.
    fn entity_filter() -> Condition {
        Condition::all().add(Expr::col((Te, TemporalEntities::Id)).eq("urn:ngsi-ld:Vehicle:1"))
    }

    fn sql_of(compiled: &CompiledTemporalQuery) -> String {
        build_aggregation_query(compiled, entity_filter()).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_period_produces_a_bucketed_group_by() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert!(sql.contains("time_bucket"), "{sql}");
        assert!(sql.contains("bucket_start"), "{sql}");
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
        assert!(!sql.contains("recorded_at"), "{sql}");
        pretty_assertions::assert_eq!(sql.matches("SELECT MIN(").count(), 1, "{sql}");
    }

    /// The bucket origin is measured over the same entity's streams, reached
    /// through the stream catalogue rather than through a current-state row.
    #[test]
    fn the_bucket_origin_is_correlated_through_the_stream_catalogue() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        assert!(sql.contains("\"s2\".\"temporal_entity_key\" = \"s\".\"temporal_entity_key\""), "{sql}");
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
        pretty_assertions::assert_eq!(group_by, "\"an\".\"name\", \"s\".\"dataset_id\", \"bucket_start\"", "{sql}");

        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], None));
        let group_by = sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        let group_by = group_by.split("ORDER BY").next().expect("a clause before ORDER BY").trim();
        pretty_assertions::assert_eq!(group_by, "\"an\".\"name\", \"s\".\"dataset_id\"", "{sql}");
    }

    /// The unit, the attribute family and the value shape all used to key the
    /// group, which silently split a period whenever one of them flipped inside
    /// it. None of them may appear in the key again.
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
    /// no longer keys them.
    #[test]
    fn the_attribute_family_and_value_shape_are_projected_as_the_span_of_the_group() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Avg], Some("PT1H")));
        for projection in [
            "MIN(\"ah\".\"attribute_kind\") AS \"attribute_kind_lowest\"",
            "MAX(\"ah\".\"attribute_kind\") AS \"attribute_kind_highest\"",
            "MIN(\"ah\".\"value_kind\") AS \"value_kind_lowest\"",
            "MAX(\"ah\".\"value_kind\") AS \"value_kind_highest\"",
        ] {
            assert!(sql.contains(projection), "{projection} missing: {sql}");
        }
        assert!(!sql.contains("attribute_type"), "{sql}");
        assert!(!sql.contains("value_type"), "{sql}");
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
        assert!(sql.contains("\"ah\".\"value_bool\"::integer::numeric"), "{sql}");
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
        pretty_assertions::assert_eq!(order_by, "\"an\".\"name\" ASC, \"s\".\"dataset_id\" ASC, \"bucket_start\" ASC", "{sql}");
    }

    #[test]
    fn min_and_max_project_a_typed_column_per_kind() {
        let sql = sql_of(&aggregated(vec![AggrMethod::Min, AggrMethod::Max], None));
        for column in ["agg_min", "agg_min_str", "agg_min_dt", "agg_max", "agg_max_str", "agg_max_dt"] {
            assert!(sql.contains(column), "{column} missing: {sql}");
        }
    }
}
