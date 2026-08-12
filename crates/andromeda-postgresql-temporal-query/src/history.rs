//! Stage 2 of a temporal query: attribute history for one entity, optionally
//! narrowed to the last `N` instances per attribute.
//!
//! The statement reads the rebuilt hypertable through the temporal catalogue
//! alone - `attribute_history` to `temporal_attribute_streams` to
//! `temporal_entities` - so an entity whose current state was deleted (clause
//! 5.6.6) still answers with its evolution. Every row carries its own
//! `attribute_kind` and `value_kind`, which is what the reconstruction reads:
//! a historical instance's kind is a fact of the row, never re-derived from a
//! catalogue that a later type change would have overwritten.

use crate::aggregation::build_aggregation_query_sql;
use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use andromeda_postgresql_query::{
    bind::{BindBuilder, BindValue},
    built::BuiltQuery,
    error::RenderError,
    time_column::{history_column, unprefixed_column},
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
    let mut conditions = vec![format!("\"te\".\"id\" = {}", binds.bind(BindValue::String(Cow::Owned(entity_id.to_string()))))];

    // Clause 5.2.21: an instance is placed on the requested axis by the column
    // that holds it. `deletedAt` exists only on a row that records a deletion,
    // so asking for that axis asks for those rows and for no others; the
    // deletion is recognised by the axis itself rather than by a sentinel value
    // stored in a value column.
    if compiled.window.timeproperty == TimeProp::Deleted {
        conditions.push("\"ah\".\"deleted_at\" IS NOT NULL".to_string());
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
        DatasetFilter::DefaultOnly => conditions.push("\"s\".\"dataset_id\" IS NULL".to_string()),
        DatasetFilter::NamedOnly(named) => {
            let marker = binds.bind(BindValue::StringVec(Cow::Owned(named.iter().map(ToString::to_string).collect())));
            conditions.push(format!("\"s\".\"dataset_id\" = ANY({marker})"));
        }
        DatasetFilter::DefaultOrNamed(named) => {
            let marker = binds.bind(BindValue::StringVec(Cow::Owned(named.iter().map(ToString::to_string).collect())));
            conditions.push(format!("(\"s\".\"dataset_id\" = ANY({marker}) OR \"s\".\"dataset_id\" IS NULL)"));
        }
    }

    let where_clause = conditions.join(" AND ");

    match compiled.representation.format {
        TemporalFormat::AggregatedValues => build_aggregation_query_sql(compiled, &where_clause, binds),
        TemporalFormat::Normalized | TemporalFormat::TemporalValues => build_history_query_sql(compiled, &where_clause, binds),
    }
}

/// The catalogue path from the hypertable to the entity grain, shared by the
/// ranked and unranked shapes so neither can reach a row the other cannot.
pub(crate) const HISTORY_JOINS: &str = r#"FROM "attribute_history" AS "ah"
JOIN "temporal_attribute_streams" AS "s" ON "s"."id" = "ah"."stream_id"
JOIN "temporal_entities" AS "te" ON "te"."entity_key" = "s"."temporal_entity_key"
JOIN "attribute_names" AS "an" ON "an"."id" = "s"."attribute_name_id""#;

/// Columns every history row carries, before the optional `lastN` ranking.
///
/// The two instance-identity columns are projected side by side: a
/// broker-minted identifier is a `UUID` rendered through the permanent URN
/// template, a client-supplied one (clause 4.5.7 requires it to survive) is the
/// stored text, and exactly one of them is populated on every row.
const BASE_COLUMNS: &str = r#""ah"."recorded_at", "ah"."stream_id",
       "ah"."attribute_kind", "ah"."value_kind",
       "ah"."observed_at", "ah"."created_at", "ah"."modified_at", "ah"."deleted_at",
       "ah"."generated_instance_uuid", "ah"."verbatim_instance_id",
       "ah"."value_number", "ah"."value_string", "ah"."value_bool",
       ST_AsEWKB("ah"."value_geo") as value_geo_ewkb,
       "ah"."value_object", "ah"."value_datetime", "ah"."value_uri",
       "ah"."unit_code_id", "ah"."metadata",
       "ah"."write_id", "ah"."write_ordinal",
       "an"."name" AS attribute_name, "s"."dataset_id""#;

/// The same columns as [`BASE_COLUMNS`], read back from the ranking CTE where
/// they are already unqualified.
const RANKED_COLUMNS: &str = r#""recorded_at", "stream_id", "attribute_kind", "value_kind",
       "observed_at", "created_at", "modified_at", "deleted_at",
       "generated_instance_uuid", "verbatim_instance_id",
       "value_number", "value_string", "value_bool", "value_geo_ewkb",
       "value_object", "value_datetime", "value_uri",
       "unit_code_id", "metadata", "write_id", "write_ordinal",
       "attribute_name", "dataset_id""#;

/// Total order over the instances of one stream, for the rankings and orderings
/// that must not depend on the plan.
///
/// `(recorded_at, write_id, write_ordinal)` is the hypertable's unique key, so
/// it separates every pair of rows, including two instances that share a value
/// on the requested time axis.
const WRITE_IDENTITY_DESC: &str = "\"ah\".\"recorded_at\" DESC, \"ah\".\"write_id\" DESC, \"ah\".\"write_ordinal\" DESC";

/// The same total order, ascending, for the rendering order of clause 5.6.11.
const WRITE_IDENTITY_ASC: &str = "\"ah\".\"recorded_at\" ASC, \"ah\".\"write_id\" ASC, \"ah\".\"write_ordinal\" ASC";

/// The ascending total order read back from the ranking CTE.
const RANKED_WRITE_IDENTITY_ASC: &str = "\"recorded_at\" ASC, \"write_id\" ASC, \"write_ordinal\" ASC";

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
    SELECT {BASE_COLUMNS},
           ROW_NUMBER() OVER (PARTITION BY "ah"."stream_id" ORDER BY {time_col} DESC, {WRITE_IDENTITY_DESC}) as rn,
           COUNT(*) OVER (PARTITION BY "ah"."stream_id") as partition_total
    {HISTORY_JOINS}
    WHERE {where_clause}
)
SELECT {RANKED_COLUMNS}, "partition_total"
FROM ranked WHERE "rn" <= {rn_marker}
ORDER BY "attribute_name", {time_col_unprefixed} ASC, {RANKED_WRITE_IDENTITY_ASC}"#
        )
    } else {
        format!(
            r#"SELECT {BASE_COLUMNS}
{HISTORY_JOINS}
WHERE {where_clause}
ORDER BY "an"."name", {time_col} ASC, {WRITE_IDENTITY_ASC}"#
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
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    /// The one entity these tests build a history statement for.
    fn vehicle_id() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid entity id")
    }

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
        assert!(result.sql.contains("\"te\".\"id\" = $1"), "{}", result.sql);
        assert!(result.sql.contains("attribute_history"), "{}", result.sql);
        assert!(result.sql.contains("JOIN \"attribute_names\""), "{}", result.sql);
        assert!(result.sql.contains("ORDER BY"), "{}", result.sql);
    }

    /// Clause 5.6.6 leaves the evolution standing when the current entity is
    /// deleted, so stage 2 reaches the entity grain through the temporal
    /// catalogue and names neither `entities` nor the retired one.
    #[test]
    fn history_reaches_the_entity_through_the_temporal_catalogue() {
        let compiled = minimal_compiled();
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(
            sql.contains("JOIN \"temporal_attribute_streams\" AS \"s\" ON \"s\".\"id\" = \"ah\".\"stream_id\""),
            "{sql}"
        );
        assert!(
            sql.contains("JOIN \"temporal_entities\" AS \"te\" ON \"te\".\"entity_key\" = \"s\".\"temporal_entity_key\""),
            "{sql}"
        );
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
        assert!(!sql.contains("\"entities\" AS"), "{sql}");
    }

    /// The kind of a historical instance is a fact of the row, because the
    /// catalogue records only the current kind of the stream and would
    /// reinterpret older rows through it.
    #[test]
    fn every_row_carries_its_own_attribute_and_value_kind() {
        let compiled = minimal_compiled();
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"ah\".\"attribute_kind\""), "{sql}");
        assert!(sql.contains("\"ah\".\"value_kind\""), "{sql}");
        assert!(!sql.contains("attribute_type"), "{sql}");
        assert!(!sql.contains("value_type"), "{sql}");
    }

    /// Clause 4.5.7: a client-supplied `instanceId` survives verbatim while a
    /// broker-minted one is a UUID rendered through the permanent template, so
    /// both columns are projected and the reconstruction picks the populated one.
    #[test]
    fn both_halves_of_the_instance_identity_are_projected() {
        let compiled = minimal_compiled();
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"ah\".\"generated_instance_uuid\""), "{sql}");
        assert!(sql.contains("\"ah\".\"verbatim_instance_id\""), "{sql}");
        assert!(!sql.contains("\"instance_id\""), "{sql}");
    }

    /// All four axes of clause 5.2.21 are rendered, and the operational
    /// `recorded_at` is projected for ordering only - never as a substitute for
    /// an axis the row does not carry.
    #[test]
    fn all_four_time_axes_are_projected_without_substitution() {
        let compiled = minimal_compiled();
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        for column in ["observed_at", "created_at", "modified_at", "deleted_at", "recorded_at"] {
            assert!(sql.contains(&format!("\"ah\".\"{column}\"")), "{column} missing: {sql}");
        }
        assert!(!sql.contains("COALESCE"), "{sql}");
    }

    /// Clause 5.2.21: the deletion axis exists only on the rows that record a
    /// deletion, and those are recognised by the axis rather than by a null URN
    /// stored in a value column (clauses 4.5.7 and 4.5.8 make that URN a
    /// rendering, not a stored value).
    #[test]
    fn the_deleted_at_axis_selects_the_rows_that_carry_it() {
        let mut compiled = minimal_compiled();
        compiled.window.timeproperty = TimeProp::Deleted;
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"ah\".\"deleted_at\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains("urn:ngsi-ld:null"), "{sql}");
    }

    #[test]
    fn last_n_ranks_on_the_write_identity_tiebreaker() {
        // Two instances sharing a timestamp must be kept or dropped consistently
        // across executions, so the ROW_NUMBER window that picks the last N
        // breaks ties on the hypertable's unique key rather than leaving it to
        // the plan.
        let mut compiled = minimal_compiled();
        compiled.instances.last_n = Some(5);
        let result = build_temporal_history_query(&vehicle_id(), &compiled).unwrap();
        assert!(result.sql.contains("ROW_NUMBER()"), "{}", result.sql);
        assert!(result.sql.contains("PARTITION BY \"ah\".\"stream_id\""), "{}", result.sql);
        assert!(result.sql.contains("\"ah\".\"write_ordinal\" DESC"), "{}", result.sql);
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

    /// Clause 4.11 defaults the selector to `observedAt`; each axis is filtered
    /// on its own column, so an instance with no `observedAt` matches no
    /// `observedAt` window.
    #[test]
    fn each_time_property_filters_its_own_column() {
        let expected = [
            (TimeProp::Observed, "\"ah\".\"observed_at\" < $2"),
            (TimeProp::Created, "\"ah\".\"created_at\" < $2"),
            (TimeProp::Modified, "\"ah\".\"modified_at\" < $2"),
            (TimeProp::Deleted, "\"ah\".\"deleted_at\" < $2"),
        ];
        for (prop, predicate) in expected {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = prop;
            compiled.window.interval = TimeInterval::Before(instant("2024-01-15T10:00:00Z"));
            let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
            assert!(sql.contains(predicate), "{prop}: {sql}");
        }
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
    /// which the stream catalogue holds as a NULL column, so it cannot be
    /// tested by the same `= ANY` predicate as a named dataset.
    #[test]
    fn dataset_id_none_selects_the_default_instance() {
        let selectors = |raw: &[&str]| -> Vec<DatasetSelector> { raw.iter().map(|entry| DatasetSelector::parse(entry).expect("valid")).collect() };

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none"]);
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"s\".\"dataset_id\" IS NULL"), "{sql}");
        assert!(!sql.contains("\"s\".\"dataset_id\" = ANY"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["urn:ngsi-ld:Dataset:one"]);
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"s\".\"dataset_id\" = ANY"), "{sql}");
        assert!(!sql.contains("\"s\".\"dataset_id\" IS NULL"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none", "urn:ngsi-ld:Dataset:one"]);
        let sql = build_temporal_history_query(&vehicle_id(), &compiled).unwrap().sql;
        assert!(sql.contains("\"s\".\"dataset_id\" = ANY"), "{sql}");
        assert!(sql.contains("OR \"s\".\"dataset_id\" IS NULL"), "{sql}");
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
