//! Aggregated temporal values: `time_bucket` windows and the aggregate
//! expressions of clause 4.5.19.

use crate::{
    error::QueryError,
    sql::{
        bind::{BindBuilder, BindValue},
        built::BuiltQuery,
    },
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_domain::temporal::AggrMethod;
use std::borrow::Cow;

/// Unified numeric expression for type-aware aggregation (NGSI-LD 4.5.19.1).
///
/// Booleans count as 0/1 and arrays as their length, so `avg` and `sum` mean
/// something for attributes the client did not write as numbers.
const NUMERIC_EXPR: &str = r#"COALESCE(
        "ah"."value_number",
        "ah"."value_bool"::int::float8,
        CASE WHEN jsonb_typeof("ah"."value_object") = 'array'
            THEN jsonb_array_length("ah"."value_object")::float8
            ELSE NULL
        END)"#;

/// Build aggregation query SQL with time_bucket.
pub fn build_aggregation_query_sql(
    compiled: &CompiledTemporalQuery,
    where_clause: &str,
    mut binds: BindBuilder<BindValue<'static>>,
) -> Result<BuiltQuery<'static>, QueryError> {
    // Clause 4.5.19: an absent period aggregates the whole range into one
    // bucket rather than bucketing at all. A zero period is absent by
    // construction, so there is nothing to test for here.
    let period = compiled.representation.aggr_period_duration;

    let (group_expr, order_expr) = if let Some(period) = period {
        let period_marker = binds.bind(BindValue::String(Cow::Owned(period.as_interval_literal())));

        // Use time_at as bucket origin when provided; otherwise use the earliest
        // data point so buckets align to the actual data range (not epoch).
        let bucket_expr = if let Some(time_at) = &compiled.window.time_at {
            let origin_marker = binds.bind(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
            format!("time_bucket({}::interval, \"ah\".\"time\", {}::timestamptz)", period_marker, origin_marker)
        } else {
            // Subquery origin: earliest ah.time in this entity's data set
            format!(
                "time_bucket({}::interval, \"ah\".\"time\", (SELECT MIN(ah2.time) FROM attribute_history ah2 JOIN temporal_entity_attributes tea2 ON tea2.id = ah2.tea_id WHERE tea2.entity_id = \"tea\".\"entity_id\"))",
                period_marker
            )
        };

        (
            format!(
                "{} AS bucket_start,\n    {} + {}::interval AS bucket_end",
                bucket_expr, bucket_expr, period_marker
            ),
            "\"an\".\"name\", bucket_start".to_string(),
        )
    } else {
        (
            "NULL::timestamptz AS bucket_start, NULL::timestamptz AS bucket_end".to_string(),
            "\"an\".\"name\"".to_string(),
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

    let group_by = if period.is_some() {
        "\"an\".\"name\", \"tea\".\"attribute_type\", \"tea\".\"dataset_id\", \"tea\".\"value_type\", bucket_start"
    } else {
        "\"an\".\"name\", \"tea\".\"attribute_type\", \"tea\".\"dataset_id\", \"tea\".\"value_type\""
    };

    let sql = format!(
        r#"SELECT
    "an"."name" AS attribute_name,
    "tea"."attribute_type",
    "tea"."dataset_id",
    "tea"."value_type",
    {},
    {}
FROM "attribute_history" AS "ah"
JOIN "temporal_entity_attributes" AS "tea" ON "tea"."id" = "ah"."tea_id"
JOIN "attribute_names" AS "an" ON "an"."id" = "tea"."attribute_name_id"
WHERE {}
GROUP BY {}
ORDER BY {}"#,
        group_expr, agg_select, where_clause, group_by, order_expr
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
    use crate::temporal::{
        aggregation_period::AggregationPeriod,
        entity_selection::EntitySelection,
        instance_selection::InstanceSelection,
        representation::Representation,
        window::TemporalWindow,
    };
    use andromeda_domain::{
        sys_attrs::SysAttrsInclusion,
        temporal::{TemporalFormat, TimeProp},
    };

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
        let built = build_aggregation_query_sql(&compiled, "\"tea\".\"entity_id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("time_bucket"), "{}", built.sql);
        assert!(built.sql.contains("bucket_start"), "{}", built.sql);
    }

    /// Clause 4.5.19: an empty or zero period aggregates the whole range into
    /// one bucket, which is what an absent period means, so no `time_bucket`
    /// is emitted for any of them.
    #[test]
    fn an_aggregation_period_of_zero_covers_the_whole_range() {
        for period in ["", "PT0S", "P0D"] {
            let compiled = aggregated(vec![AggrMethod::TotalCount], Some(period));
            let built = build_aggregation_query_sql(&compiled, "\"tea\".\"entity_id\" = $1", BindBuilder::default()).unwrap();
            assert!(!built.sql.contains("time_bucket"), "{period}: {}", built.sql);
            assert!(built.sql.contains("NULL::timestamptz AS bucket_start"), "{period}: {}", built.sql);
        }
    }

    #[test]
    fn an_empty_method_list_still_counts() {
        let compiled = aggregated(Vec::new(), None);
        let built = build_aggregation_query_sql(&compiled, "\"tea\".\"entity_id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("agg_total_count"), "{}", built.sql);
    }

    #[test]
    fn min_and_max_project_a_typed_column_per_kind() {
        let compiled = aggregated(vec![AggrMethod::Min, AggrMethod::Max], None);
        let built = build_aggregation_query_sql(&compiled, "\"tea\".\"entity_id\" = $1", BindBuilder::default()).unwrap();
        for column in ["agg_min", "agg_min_str", "agg_min_dt", "agg_max", "agg_max_str", "agg_max_dt"] {
            assert!(built.sql.contains(column), "{column} missing: {}", built.sql);
        }
    }
}
