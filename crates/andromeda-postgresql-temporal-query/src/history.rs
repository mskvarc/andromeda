//! Stage 2 of a temporal query: attribute history for one entity, optionally
//! narrowed to the last `N` instances per attribute.

use crate::{
    aggregation::build_aggregation_query_sql,
    time_column::{history_column, unprefixed_column},
};
use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use andromeda_postgresql_query::{
    bind::{BindBuilder, BindValue},
    built::BuiltQuery,
    error::RenderError,
};
use std::borrow::Cow;

/// Build Stage 2 query: temporal history for a single entity.
///
/// The entity id is bound here rather than by the caller, so the statement owns
/// its whole placeholder sequence and no two functions have to agree on where
/// it starts.
pub fn build_temporal_history_query(entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<BuiltQuery<'static>, RenderError> {
    let time_col = history_column(compiled.window.timeproperty);
    let mut binds = BindBuilder::default();
    let mut conditions = vec![format!(
        "\"tea\".\"entity_id\" = {}",
        binds.bind(BindValue::String(Cow::Owned(entity_id.to_string())))
    )];

    // When timeproperty=deletedAt, only return tombstone instances
    if compiled.window.timeproperty == TimeProp::Deleted {
        conditions.push("\"ah\".\"value_string\" = 'urn:ngsi-ld:null'".to_string());
    }

    match compiled.window.interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => {
            let marker = binds.bind(BindValue::Timestamp(instant));
            conditions.push(format!("{time_col} < {marker}"));
        }
        TimeInterval::After(instant) => {
            let marker = binds.bind(BindValue::Timestamp(instant));
            conditions.push(format!("{time_col} >= {marker}"));
        }
        TimeInterval::Between { from, to } => {
            let start = binds.bind(BindValue::Timestamp(from));
            conditions.push(format!("{time_col} >= {start}"));
            let end = binds.bind(BindValue::Timestamp(to));
            conditions.push(format!("{time_col} < {end}"));
        }
    }

    if !compiled.instances.attr_filter.is_empty() {
        let names: Vec<String> = compiled.instances.attr_filter.iter().map(|iri| iri.as_str().to_string()).collect();
        let marker = binds.bind(BindValue::StringVec(Cow::Owned(names)));
        conditions.push(format!("\"an\".\"name\" = ANY({marker})"));
    }

    // datasetId filter (clause 4.5.5): the default instance is stored as a
    // NULL `dataset_id`, so it is tested separately from the named ones.
    match DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter) {
        DatasetFilter::Unfiltered => {}
        DatasetFilter::DefaultOnly => conditions.push("\"tea\".\"dataset_id\" IS NULL".to_string()),
        DatasetFilter::NamedOnly(named) => {
            let marker = binds.bind(BindValue::StringVec(Cow::Owned(named.iter().map(ToString::to_string).collect())));
            conditions.push(format!("\"tea\".\"dataset_id\" = ANY({marker})"));
        }
        DatasetFilter::DefaultOrNamed(named) => {
            let marker = binds.bind(BindValue::StringVec(Cow::Owned(named.iter().map(ToString::to_string).collect())));
            conditions.push(format!("(\"tea\".\"dataset_id\" = ANY({marker}) OR \"tea\".\"dataset_id\" IS NULL)"));
        }
    }

    let where_clause = conditions.join(" AND ");

    match compiled.representation.format {
        TemporalFormat::AggregatedValues => build_aggregation_query_sql(compiled, &where_clause, binds),
        TemporalFormat::Normalized | TemporalFormat::TemporalValues => build_history_query_sql(compiled, &where_clause, binds),
    }
}

/// Columns every history row carries, before the optional `lastN` ranking.
const BASE_COLUMNS: &str = r#""ah"."time", "ah"."tea_id",
       "ah"."value_number", "ah"."value_string", "ah"."value_bool",
       ST_AsEWKB("ah"."value_geo") as value_geo_ewkb,
       "ah"."value_object", "ah"."value_datetime", "ah"."value_uri",
       "ah"."instance_id", "ah"."observed_at", "ah"."metadata",
       "ah"."recorded_at", COALESCE("ah"."modified_at", "ah"."recorded_at") AS modified_at,
       "ah"."intent_id", "ah"."intent_ordinal",
       "an"."name" AS attribute_name, "tea"."attribute_type", "tea"."dataset_id", "tea"."value_type""#;

/// Build history query SQL (with optional lastN CTE).
fn build_history_query_sql(
    compiled: &CompiledTemporalQuery,
    where_clause: &str,
    mut binds: BindBuilder<BindValue<'static>>,
) -> Result<BuiltQuery<'static>, RenderError> {
    let time_col = history_column(compiled.window.timeproperty);
    let time_col_unprefixed = unprefixed_column(compiled.window.timeproperty);

    let sql = if let Some(last_n) = compiled.instances.last_n {
        let rn_marker = binds.bind(BindValue::I64(last_n as i64));

        format!(
            r#"WITH ranked AS (
    SELECT {},
           ROW_NUMBER() OVER (PARTITION BY "ah"."tea_id" ORDER BY {} DESC, "ah"."instance_id" DESC) as rn,
           COUNT(*) OVER (PARTITION BY "ah"."tea_id") as partition_total
    FROM "attribute_history" AS "ah"
    JOIN "temporal_entity_attributes" AS "tea" ON "tea"."id" = "ah"."tea_id"
    JOIN "attribute_names" AS "an" ON "an"."id" = "tea"."attribute_name_id"
    WHERE {}
)
SELECT "time", "tea_id", "value_number", "value_string", "value_bool", "value_geo_ewkb",
       "value_object", "value_datetime", "value_uri", "instance_id", "observed_at", "metadata", "recorded_at", "modified_at", "intent_id", "intent_ordinal",
       "attribute_name", "attribute_type", "dataset_id", "value_type", "partition_total"
FROM ranked WHERE "rn" <= {}
ORDER BY "attribute_name", {} ASC, "instance_id" ASC"#,
            BASE_COLUMNS, time_col, where_clause, rn_marker, time_col_unprefixed
        )
    } else {
        format!(
            r#"SELECT {}
FROM "attribute_history" AS "ah"
JOIN "temporal_entity_attributes" AS "tea" ON "tea"."id" = "ah"."tea_id"
JOIN "attribute_names" AS "an" ON "an"."id" = "tea"."attribute_name_id"
WHERE {}
ORDER BY "an"."name", {} ASC"#,
            BASE_COLUMNS, where_clause, time_col
        )
    };

    Ok(BuiltQuery {
        sql,
        count_sql: None,
        bind_values: binds.into_values(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        entity_selection::EntitySelection,
        temporal::{aggregation_period::AggregationPeriod, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    /// The one entity these tests build a history statement for.
    fn vehicle_id() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid entity id")
    }
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;

    /// A query with every filter absent, for tests that vary one part at a time.
    fn minimal_compiled() -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    #[test]
    fn build_temporal_history_no_filters() {
        let compiled = minimal_compiled();
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("\"tea\".\"entity_id\" = $1"));
        assert!(result.sql.contains("attribute_history"));
        assert!(result.sql.contains("JOIN \"attribute_names\""));
        assert!(result.sql.contains("ORDER BY"));
    }

    #[test]
    fn last_n_ranks_with_an_instance_id_tiebreaker() {
        // Two instances sharing a timestamp must be kept or dropped consistently
        // across executions, so the ROW_NUMBER window that picks the last N
        // breaks ties on instance_id rather than leaving it to the plan.
        let mut compiled = minimal_compiled();
        compiled.instances.last_n = Some(5);
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("ROW_NUMBER()"), "{}", result.sql);
        assert!(result.sql.contains("\"ah\".\"instance_id\" DESC"), "{}", result.sql);
    }

    #[test]
    fn build_temporal_history_before() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::Before(instant("2024-01-15T10:00:00Z"));
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("< $2"));
        assert!(matches!(result.bind_values[1], BindValue::Timestamp(_)));
    }

    #[test]
    fn build_temporal_history_after() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-15T10:00:00Z"));
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains(">= $2"));
        assert!(matches!(result.bind_values[1], BindValue::Timestamp(_)));
    }

    #[test]
    fn build_temporal_history_between() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::Between {
            from: instant("2024-01-15T10:00:00Z"),
            to: instant("2024-01-15T12:00:00Z"),
        };
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains(">="));
        assert!(result.sql.contains("<"));
    }

    #[test]
    fn build_temporal_history_with_attrs() {
        let mut compiled = minimal_compiled();
        compiled.instances.attr_filter = vec![IriBuf::new("https://uri.etsi.org/ngsi-ld/default-context/speed".to_string()).expect("valid IRI")];
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("\"an\".\"name\" = ANY($2)"));
    }

    #[test]
    fn build_temporal_history_with_last_n() {
        let mut compiled = minimal_compiled();
        compiled.instances.last_n = Some(5);
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("WITH ranked"));
        assert!(result.sql.contains("ROW_NUMBER()"));
        assert!(result.sql.contains("\"rn\" <="));
    }

    /// Placeholders in a hand-written statement have to be contiguous from
    /// `$1`: a gap binds the wrong value to a predicate, and a repeat binds one
    /// value to two. Before [`BindBuilder`], the numbering was arithmetic three
    /// functions agreed on and nothing checked.
    fn placeholder_indices(sql: &str) -> Vec<usize> {
        let mut found = Vec::new();
        for (offset, byte) in sql.bytes().enumerate() {
            if byte == b'$' {
                let digits: String = sql[offset + 1..].chars().take_while(char::is_ascii_digit).collect();
                if let Ok(index) = digits.parse::<usize>() {
                    found.push(index);
                }
            }
        }
        found
    }

    #[test]
    fn stage_two_placeholders_are_contiguous_from_one() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::Between {
            from: instant("2024-01-15T10:00:00Z"),
            to: instant("2024-01-15T12:00:00Z"),
        };
        compiled.instances.attr_filter = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        compiled.instances.dataset_id_filter = vec![DatasetSelector::parse("urn:ngsi-ld:Dataset:one").expect("valid selector")];
        compiled.instances.last_n = Some(5);

        let built = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        let mut indices = placeholder_indices(&built.sql);
        indices.sort_unstable();
        indices.dedup();
        let expected: Vec<usize> = (1..=built.bind_values.len()).collect();
        pretty_assertions::assert_eq!(indices, expected, "{}", built.sql);
    }

    /// Clause 4.5.5: `@none` selects the instance that carries no `datasetId`,
    /// which the store holds as a NULL column, so it cannot be tested by the
    /// same `= ANY` predicate as a named dataset.
    #[test]
    fn dataset_id_none_selects_the_default_instance() {
        let selectors = |raw: &[&str]| -> Vec<DatasetSelector> { raw.iter().map(|entry| DatasetSelector::parse(entry).expect("valid")).collect() };

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none"]);
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"tea\".\"dataset_id\" IS NULL"), "{sql}");
        assert!(!sql.contains("\"tea\".\"dataset_id\" = ANY"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["urn:ngsi-ld:Dataset:one"]);
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"tea\".\"dataset_id\" = ANY"), "{sql}");
        assert!(!sql.contains("\"tea\".\"dataset_id\" IS NULL"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none", "urn:ngsi-ld:Dataset:one"]);
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"tea\".\"dataset_id\" = ANY"), "{sql}");
        assert!(sql.contains("OR \"tea\".\"dataset_id\" IS NULL"), "{sql}");
    }

    #[test]
    fn stage_two_placeholders_are_contiguous_when_aggregating() {
        let mut compiled = minimal_compiled();
        compiled.representation.format = TemporalFormat::AggregatedValues;
        compiled.representation.aggr_methods = vec![AggrMethod::Avg];
        compiled.representation.aggr_period_duration = AggregationPeriod::parse("PT1H").expect("valid period");
        compiled.window.interval = TimeInterval::After(instant("2024-01-15T10:00:00Z"));
        compiled.instances.attr_filter = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];

        let built = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        let mut indices = placeholder_indices(&built.sql);
        indices.sort_unstable();
        indices.dedup();
        let expected: Vec<usize> = (1..=built.bind_values.len()).collect();
        pretty_assertions::assert_eq!(indices, expected, "{}", built.sql);
    }

    #[test]
    fn build_aggregation_query_with_period() {
        let mut compiled = minimal_compiled();
        compiled.representation.format = TemporalFormat::AggregatedValues;
        compiled.representation.aggr_methods = vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max];
        compiled.representation.aggr_period_duration = AggregationPeriod::parse("PT1H").expect("valid period");
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("time_bucket"));
        assert!(result.sql.contains("bucket_start"));
        assert!(result.sql.contains("agg_avg"));
        assert!(result.sql.contains("JOIN \"attribute_names\""));
    }

    #[test]
    fn build_aggregation_query_whole_range() {
        let mut compiled = minimal_compiled();
        compiled.representation.format = TemporalFormat::AggregatedValues;
        compiled.representation.aggr_methods = vec![AggrMethod::TotalCount];
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("agg_total_count"));
        assert!(result.sql.contains("GROUP BY"));
    }
}
