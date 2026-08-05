//! Stage 2 of a temporal query: attribute history for one entity, optionally
//! narrowed to the last `N` instances per attribute.

use crate::{
    error::QueryError,
    sql::{
        bind::{BindBuilder, BindValue},
        built::BuiltQuery,
    },
    temporal::{
        compiled::CompiledTemporalQuery,
        dataset::DatasetFilter,
        sql::aggregation::build_aggregation_query_sql,
        window::{history_column, unprefixed_column},
    },
};
use andromeda_domain::temporal::{TemporalFormat, TimeProp, TimeRel};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use std::borrow::Cow;
use urn_rs::Urn;

/// Build Stage 2 query: temporal history for a single entity.
///
/// The entity id is bound here rather than by the caller, so the statement owns
/// its whole placeholder sequence and no two functions have to agree on where
/// it starts.
pub fn build_temporal_history_query(entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<BuiltQuery<'static>, QueryError> {
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

    if let (Some(timerel), Some(time_at)) = (&compiled.window.timerel, &compiled.window.time_at) {
        let lower_bound = BindValue::String(Cow::Owned(time_at.to_rfc3339()));
        match timerel {
            TimeRel::Before => conditions.push(format!("{} < {}::timestamptz", time_col, binds.bind(lower_bound))),
            TimeRel::After => conditions.push(format!("{} >= {}::timestamptz", time_col, binds.bind(lower_bound))),
            TimeRel::Between => {
                let end_time = compiled.window.end_time_at.as_ref().ok_or(QueryError::ParamEndTimeAtRequired)?;
                conditions.push(format!("{} >= {}::timestamptz", time_col, binds.bind(lower_bound)));
                let upper_bound = BindValue::String(Cow::Owned(end_time.to_rfc3339()));
                conditions.push(format!("{} < {}::timestamptz", time_col, binds.bind(upper_bound)));
            }
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
            let marker = binds.bind(BindValue::StringVec(Cow::Owned(named.iter().map(Urn::to_string).collect())));
            conditions.push(format!("\"tea\".\"dataset_id\" = ANY({marker})"));
        }
        DatasetFilter::DefaultOrNamed(named) => {
            let marker = binds.bind(BindValue::StringVec(Cow::Owned(named.iter().map(Urn::to_string).collect())));
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
       "an"."name" AS attribute_name, "tea"."attribute_type", "tea"."dataset_id", "tea"."value_type""#;

/// Build history query SQL (with optional lastN CTE).
fn build_history_query_sql(
    compiled: &CompiledTemporalQuery,
    where_clause: &str,
    mut binds: BindBuilder<BindValue<'static>>,
) -> Result<BuiltQuery<'static>, QueryError> {
    let time_col = history_column(compiled.window.timeproperty);
    let time_col_unprefixed = unprefixed_column(compiled.window.timeproperty);

    let sql = if let Some(last_n) = compiled.instances.last_n {
        let rn_marker = binds.bind(BindValue::I64(last_n as i64));

        format!(
            r#"WITH ranked AS (
    SELECT {},
           ROW_NUMBER() OVER (PARTITION BY "ah"."tea_id" ORDER BY {} DESC) as rn,
           COUNT(*) OVER (PARTITION BY "ah"."tea_id") as partition_total
    FROM "attribute_history" AS "ah"
    JOIN "temporal_entity_attributes" AS "tea" ON "tea"."id" = "ah"."tea_id"
    JOIN "attribute_names" AS "an" ON "an"."id" = "tea"."attribute_name_id"
    WHERE {}
)
SELECT "time", "tea_id", "value_number", "value_string", "value_bool", "value_geo_ewkb",
       "value_object", "value_datetime", "value_uri", "instance_id", "observed_at", "metadata",
       "attribute_name", "attribute_type", "dataset_id", "value_type", "partition_total"
FROM ranked WHERE "rn" <= {}
ORDER BY "attribute_name", {} ASC"#,
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
    use crate::temporal::{
        aggregation_period::AggregationPeriod,
        dataset::DatasetSelector,
        entity_selection::EntitySelection,
        instance_selection::InstanceSelection,
        representation::Representation,
        window::TemporalWindow,
    };
    use andromeda_domain::{
        sys_attrs::SysAttrsInclusion,
        temporal::{AggrMethod, TemporalFormat, TimeProp},
    };

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
    fn build_temporal_history_before() {
        let mut compiled = minimal_compiled();
        compiled.window.timerel = Some(TimeRel::Before);
        compiled.window.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("< $2::timestamptz"));
    }

    #[test]
    fn build_temporal_history_after() {
        let mut compiled = minimal_compiled();
        compiled.window.timerel = Some(TimeRel::After);
        compiled.window.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains(">= $2::timestamptz"));
    }

    #[test]
    fn build_temporal_history_between() {
        let mut compiled = minimal_compiled();
        compiled.window.timerel = Some(TimeRel::Between);
        compiled.window.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        compiled.window.end_time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T12:00:00Z").unwrap().with_timezone(&Utc));
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
        compiled.window.timerel = Some(TimeRel::Between);
        compiled.window.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        compiled.window.end_time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T12:00:00Z").unwrap().with_timezone(&Utc));
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
        compiled.window.timerel = Some(TimeRel::After);
        compiled.window.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
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
