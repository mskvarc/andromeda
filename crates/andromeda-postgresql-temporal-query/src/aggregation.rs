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

use crate::history::HISTORY_JOINS;
use andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::{
    bind::{BindBuilder, BindValue},
    built::BuiltQuery,
    error::RenderError,
    time_column::{history_column, history_column_for_alias},
};
use std::borrow::Cow;

/// Unified numeric expression for type-aware aggregation (NGSI-LD 4.5.19.1).
///
/// Booleans count as 0/1 and arrays as their length, so `avg` and `sum` mean
/// something for attributes the client did not write as numbers.
///
/// Every branch is `NUMERIC`, the type the value column itself carries: a
/// JSON number is an exact decimal (clause 4.5.2 gives no precision limit), and
/// widening it to binary floating point to aggregate would report a minimum or
/// a maximum that was never observed.
const NUMERIC_EXPR: &str = r#"COALESCE(
        "ah"."value_number",
        "ah"."value_bool"::int::numeric,
        CASE WHEN jsonb_typeof("ah"."value_object") = 'array'
            THEN jsonb_array_length("ah"."value_object")::numeric
            ELSE NULL
        END)"#;

/// The kind code reserved for a deletion (clause 4.5.7), excluded from every
/// aggregate.
///
/// A tombstone records that an attribute was deleted and carries no value at
/// all, so counting it as an observation would report a value where the
/// evolution says there is none.
const TOMBSTONE_EXCLUSION: &str = r#""ah"."value_kind" <> 0"#;

/// The persisted attribute family of an observation (clause 5.2.4).
///
/// Projected as its own extrema rather than grouped on: the family lives on the
/// row rather than in the catalogue, so an attribute whose type changed within
/// a period is one period whose rows disagree, not two periods.
const ATTRIBUTE_KIND_COLUMN: &str = r#""ah"."attribute_kind""#;

/// The persisted unit identity of an observation (clause 4.5.2 `unitCode`).
///
/// Projected as its own extrema, plus whether any row carried no unit at all:
/// `MIN` and `MAX` skip nulls, so the two bounds alone cannot tell an
/// all-annotated group from one that mixed annotated and unannotated rows.
const UNIT_CODE_COLUMN: &str = r#""ah"."unit_code_id""#;

/// The value shape an aggregated group's observations were recorded under.
///
/// Projected as its own extrema: `min` and `max` are read from the typed column
/// the shape names, so the reader needs the shape the group's rows share and
/// needs to be told when they share none.
const VALUE_KIND_COLUMN: &str = r#""ah"."value_kind""#;

/// The span of registry codes and unit identities one aggregation group covered.
///
/// Every entry is an aggregate over the group rather than a group key, which is
/// what keeps the group key at the attribute, dataset and period clause 4.5.19
/// names.
fn group_span_columns() -> String {
    format!(
        r#"MIN({ATTRIBUTE_KIND_COLUMN}) AS attribute_kind_lowest,
    MAX({ATTRIBUTE_KIND_COLUMN}) AS attribute_kind_highest,
    MIN({VALUE_KIND_COLUMN}) AS value_kind_lowest,
    MAX({VALUE_KIND_COLUMN}) AS value_kind_highest,
    MIN({UNIT_CODE_COLUMN}) AS unit_code_id_lowest,
    MAX({UNIT_CODE_COLUMN}) AS unit_code_id_highest,
    BOOL_OR({UNIT_CODE_COLUMN} IS NULL) AS any_unitless"#
    )
}

/// Build aggregation query SQL with time_bucket.
pub fn build_aggregation_query_sql(
    compiled: &CompiledTemporalQuery,
    where_clause: &str,
    mut binds: BindBuilder<BindValue<'static>>,
) -> Result<BuiltQuery<'static>, RenderError> {
    // Clause 4.5.19: an absent period aggregates the whole range into one
    // bucket rather than bucketing at all. A zero period is absent by
    // construction, so there is nothing to test for here.
    let period = compiled.representation.aggr_period_duration;
    // Bucket on the same time axis the WHERE clause filters on, which for the
    // default selector of clause 4.11 is `observed_at` alone. Bucketing on the
    // operational write clock would assign a backfilled observation to a
    // different period than the one its window filter selected, so aggregates
    // would land in the wrong periods (clause 4.5.19).
    let time_col = history_column(compiled.window.timeproperty);

    let mut origin_join = String::new();
    let (group_expr, order_expr) = if let Some(period) = period {
        let period_marker = binds.bind(BindValue::String(Cow::Owned(period.as_iso8601())));

        // Use time_at as bucket origin when provided; otherwise use the earliest
        // data point so buckets align to the actual data range (not epoch).
        let bucket_expr = if let Some(time_at) = compiled.window.interval.reference() {
            let origin_marker = binds.bind(BindValue::Timestamp(*time_at));
            format!("time_bucket({period_marker}::interval, {time_col}, {origin_marker})")
        } else {
            // The origin is derived once per selected entity and reused for
            // both bucket bounds, rather than evaluating the same aggregate
            // expression once for each rendered column.
            let origin_axis = history_column_for_alias(compiled.window.timeproperty, "ah2");
            origin_join = format!(
                "\nCROSS JOIN LATERAL (SELECT MIN({origin_axis}) AS origin FROM attribute_history ah2 JOIN temporal_attribute_streams s2 ON s2.id = ah2.stream_id WHERE s2.temporal_entity_key = \"s\".\"temporal_entity_key\") AS bucket_origin"
            );
            format!("time_bucket({period_marker}::interval, {time_col}, bucket_origin.origin)")
        };

        (
            format!(
                "{} AS bucket_start,\n    {} + {}::interval AS bucket_end",
                bucket_expr, bucket_expr, period_marker
            ),
            "\"an\".\"name\", \"s\".\"dataset_id\", bucket_start".to_string(),
        )
    } else {
        (
            "NULL::timestamptz AS bucket_start, NULL::timestamptz AS bucket_end".to_string(),
            "\"an\".\"name\", \"s\".\"dataset_id\"".to_string(),
        )
    };

    let mut agg_exprs = Vec::new();
    for m in &compiled.representation.aggr_methods {
        match m {
            AggrMethod::Avg => {
                agg_exprs.push(format!("AVG({}) AS agg_avg", NUMERIC_EXPR));
            }
            AggrMethod::Min => {
                agg_exprs.push(format!("MIN({}) AS agg_min", NUMERIC_EXPR));
                agg_exprs.push("MIN(\"ah\".\"value_string\") AS agg_min_str".to_string());
                agg_exprs.push("MIN(\"ah\".\"value_datetime\") AS agg_min_dt".to_string());
            }
            AggrMethod::Max => {
                agg_exprs.push(format!("MAX({}) AS agg_max", NUMERIC_EXPR));
                agg_exprs.push("MAX(\"ah\".\"value_string\") AS agg_max_str".to_string());
                agg_exprs.push("MAX(\"ah\".\"value_datetime\") AS agg_max_dt".to_string());
            }
            AggrMethod::Sum => {
                agg_exprs.push(format!("SUM({}) AS agg_sum", NUMERIC_EXPR));
            }
            AggrMethod::SumSq => {
                agg_exprs.push(format!("SUM(({}) * ({})) AS agg_sumsq", NUMERIC_EXPR, NUMERIC_EXPR));
            }
            AggrMethod::StdDev => {
                agg_exprs.push(format!("STDDEV({}) AS agg_stddev", NUMERIC_EXPR));
            }
            AggrMethod::TotalCount => {
                agg_exprs.push("COUNT(*) AS agg_total_count".to_string());
            }
            AggrMethod::DistinctCount => {
                agg_exprs.push(
                    r#"COUNT(DISTINCT COALESCE(
                        "ah"."value_number"::text, "ah"."value_string", "ah"."value_bool"::text,
                        "ah"."value_datetime"::text, "ah"."value_uri", "ah"."value_object"::text
                    )) AS agg_distinct_count"#
                        .to_string(),
                );
            }
        }
    }

    let agg_select = if agg_exprs.is_empty() {
        "COUNT(*) AS agg_total_count".to_string()
    } else {
        agg_exprs.join(",\n    ")
    };

    // Clause 4.5.19: one aggregate value per attribute, `datasetId` and period,
    // so the group key is those three and nothing else. A period is a bucket
    // and an absent period is the whole range, which is one bucket rather than
    // no grouping at all.
    let group_by = if period.is_some() {
        "\"an\".\"name\", \"s\".\"dataset_id\", bucket_start"
    } else {
        "\"an\".\"name\", \"s\".\"dataset_id\""
    };
    let span_columns = group_span_columns();

    let sql = format!(
        r#"SELECT
    "an"."name" AS attribute_name,
    "s"."dataset_id",
    {span_columns},
    {group_expr},
    {agg_select}
{HISTORY_JOINS}{origin_join}
WHERE {where_clause} AND {TOMBSTONE_EXCLUSION}
GROUP BY {group_by}
ORDER BY {order_expr}"#
    );

    Ok(BuiltQuery {
        sql,
        count_sql: None,
        bind_values: binds.into_values(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{aggregation_period::AggregationPeriod, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;

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

    #[test]
    fn a_period_produces_a_bucketed_group_by() {
        let compiled = aggregated(vec![AggrMethod::Avg], Some("PT1H"));
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("time_bucket"), "{}", built.sql);
        assert!(built.sql.contains("bucket_start"), "{}", built.sql);
    }

    #[test]
    fn observed_at_buckets_on_the_observation_axis_not_write_time() {
        // Clause 4.5.19: the bucket is computed on the same axis the window
        // filters on, and clause 4.11 makes that `observedAt` by default, so a
        // backfilled observation lands in the period its filter selected rather
        // than the one the write clock would place it in. The correlated origin
        // subquery measures the same axis under its own alias.
        let compiled = aggregated(vec![AggrMethod::Avg], Some("PT1H"));
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("time_bucket($1::interval, \"ah\".\"observed_at\""), "{}", built.sql);
        assert!(built.sql.contains("MIN(\"ah2\".\"observed_at\")"), "{}", built.sql);
        assert!(!built.sql.contains("recorded_at"), "{}", built.sql);
        pretty_assertions::assert_eq!(built.sql.matches("SELECT MIN(").count(), 1, "{}", built.sql);
    }

    /// The bucket origin is measured over the same entity's streams, reached
    /// through the stream catalogue rather than through a current-state row.
    #[test]
    fn the_bucket_origin_is_correlated_through_the_stream_catalogue() {
        let compiled = aggregated(vec![AggrMethod::Avg], Some("PT1H"));
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("s2.temporal_entity_key = \"s\".\"temporal_entity_key\""), "{}", built.sql);
        assert!(!built.sql.contains("temporal_entity_attributes"), "{}", built.sql);
    }

    /// Clause 4.5.7: a deletion carries no value, so the rows that record one
    /// are outside every aggregate rather than counted as observations.
    #[test]
    fn tombstone_rows_are_excluded_from_every_aggregate() {
        let compiled = aggregated(vec![AggrMethod::TotalCount], None);
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("\"ah\".\"value_kind\" <> 0"), "{}", built.sql);
    }

    /// Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
    /// period. A fourth group column would answer one period with one entry per
    /// value the rows happened to disagree on, so the key holds those three and
    /// no others.
    #[test]
    fn the_group_key_is_the_attribute_the_dataset_and_the_period_alone() {
        let bucketed = aggregated(vec![AggrMethod::Avg], Some("PT1H"));
        let built = build_aggregation_query_sql(&bucketed, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        let group_by = built.sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        let group_by = group_by.split("ORDER BY").next().expect("a clause before ORDER BY").trim();
        pretty_assertions::assert_eq!(group_by, "\"an\".\"name\", \"s\".\"dataset_id\", bucket_start", "{}", built.sql);

        let whole_range = aggregated(vec![AggrMethod::Avg], None);
        let built = build_aggregation_query_sql(&whole_range, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        let group_by = built.sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        let group_by = group_by.split("ORDER BY").next().expect("a clause before ORDER BY").trim();
        pretty_assertions::assert_eq!(group_by, "\"an\".\"name\", \"s\".\"dataset_id\"", "{}", built.sql);
    }

    /// The unit, the attribute family and the value shape all used to key the
    /// group, which silently split a period whenever one of them flipped inside
    /// it. None of them may appear in the key again.
    #[test]
    fn no_unit_or_registry_code_keys_the_group() {
        for period in [Some("PT1H"), None] {
            let compiled = aggregated(vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::TotalCount], period);
            let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
            let group_by = built.sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
            for column in ["unit_code_id", "value_kind", "attribute_kind"] {
                assert!(!group_by.contains(column), "{column} still keys the group: {}", built.sql);
            }
        }
    }

    /// The family is a fact of the row rather than of the mutable catalogue, so
    /// it is read from the hypertable — as the span of the group's rows, since
    /// it no longer keys them.
    #[test]
    fn the_attribute_family_and_value_shape_are_projected_as_the_span_of_the_group() {
        let compiled = aggregated(vec![AggrMethod::Avg], Some("PT1H"));
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        for projection in [
            "MIN(\"ah\".\"attribute_kind\") AS attribute_kind_lowest",
            "MAX(\"ah\".\"attribute_kind\") AS attribute_kind_highest",
            "MIN(\"ah\".\"value_kind\") AS value_kind_lowest",
            "MAX(\"ah\".\"value_kind\") AS value_kind_highest",
        ] {
            assert!(built.sql.contains(projection), "{projection} missing: {}", built.sql);
        }
        assert!(!built.sql.contains("attribute_type"), "{}", built.sql);
        assert!(!built.sql.contains("value_type"), "{}", built.sql);
    }

    /// Clause 4.5.2 attaches a `unitCode` to the instance, not to the attribute,
    /// so two instances of one attribute may differ in unit. `MIN` and `MAX`
    /// skip nulls, so the bounds alone cannot separate an all-annotated group
    /// from one that also held unannotated rows, and the null test is projected
    /// beside them.
    #[test]
    fn the_unit_span_carries_both_bounds_and_whether_any_row_carried_no_unit() {
        for period in [Some("PT1H"), None] {
            let compiled = aggregated(vec![AggrMethod::Avg], period);
            let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
            for projection in [
                "MIN(\"ah\".\"unit_code_id\") AS unit_code_id_lowest",
                "MAX(\"ah\".\"unit_code_id\") AS unit_code_id_highest",
                "BOOL_OR(\"ah\".\"unit_code_id\" IS NULL) AS any_unitless",
            ] {
                assert!(built.sql.contains(projection), "{projection} missing: {}", built.sql);
            }
        }
    }

    /// An exact decimal stays exact: the aggregate reads the `NUMERIC` value
    /// column rather than widening it to binary floating point.
    #[test]
    fn numeric_values_are_aggregated_without_a_float_cast() {
        let compiled = aggregated(vec![AggrMethod::Min, AggrMethod::Max, AggrMethod::Avg], None);
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        assert!(!built.sql.contains("float8"), "{}", built.sql);
        assert!(built.sql.contains("\"ah\".\"value_bool\"::int::numeric"), "{}", built.sql);
    }

    /// Clause 4.5.19: an empty or zero period aggregates the whole range into
    /// one bucket, which is what an absent period means, so no `time_bucket`
    /// is emitted for any of them.
    #[test]
    fn an_aggregation_period_of_zero_covers_the_whole_range() {
        for period in ["", "PT0S", "P0D"] {
            let compiled = aggregated(vec![AggrMethod::TotalCount], Some(period));
            let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
            assert!(!built.sql.contains("time_bucket"), "{period}: {}", built.sql);
            assert!(built.sql.contains("NULL::timestamptz AS bucket_start"), "{period}: {}", built.sql);
        }
    }

    #[test]
    fn an_empty_method_list_still_counts() {
        let compiled = aggregated(Vec::new(), None);
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("agg_total_count"), "{}", built.sql);
    }

    /// The renderer emits one triple per row, so the rows of one attribute and
    /// dataset have to arrive in period order rather than in whatever order the
    /// plan produced them.
    #[test]
    fn the_rows_arrive_ordered_by_attribute_dataset_and_period() {
        let compiled = aggregated(vec![AggrMethod::Avg], Some("PT1H"));
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        let order_by = built.sql.split("ORDER BY").nth(1).expect("an ORDER BY clause").trim();
        pretty_assertions::assert_eq!(order_by, "\"an\".\"name\", \"s\".\"dataset_id\", bucket_start", "{}", built.sql);
    }

    #[test]
    fn min_and_max_project_a_typed_column_per_kind() {
        let compiled = aggregated(vec![AggrMethod::Min, AggrMethod::Max], None);
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        for column in ["agg_min", "agg_min_str", "agg_min_dt", "agg_max", "agg_max_str", "agg_max_dt"] {
            assert!(built.sql.contains(column), "{column} missing: {}", built.sql);
        }
    }
}
