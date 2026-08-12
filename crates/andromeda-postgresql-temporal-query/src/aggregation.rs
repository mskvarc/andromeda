//! Aggregated temporal values: `time_bucket` windows and the aggregate
//! expressions of clause 4.5.19.
//!
//! An aggregate is also a statement about a unit. Clause 4.5.2 lets every
//! Property instance carry its own `unitCode`, and nothing in the
//! specification says what the mean of a kilogram and a pound is; Andromeda
//! never converts (`docs/unit-handling.md`), so the query groups on the unit
//! identity and a target left spanning several units is refused rather than
//! answered with a number nobody can interpret.

use crate::history::HISTORY_JOINS;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::{
    bind::{BindBuilder, BindValue},
    built::BuiltQuery,
    error::RenderError,
    time_column::{history_column, history_column_for_alias},
};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use std::{
    borrow::Cow,
    collections::{HashMap, hash_map::Entry},
};
use thiserror::Error;

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

/// The kind an aggregated group is rendered under (clause 4.5.19).
///
/// Grouped as well as projected: the kind lives on the row rather than in the
/// catalogue, so an attribute whose type changed within the window aggregates
/// as one group per kind rather than as a single group rendered under whichever
/// kind the plan happened to surface.
const ATTRIBUTE_KIND_COLUMN: &str = r#""ah"."attribute_kind""#;

/// The persisted unit identity of an observation (clause 4.5.2 `unitCode`).
///
/// Part of every aggregation group key: an aggregate whose inputs were
/// recorded in different units is not computed, and grouping is what keeps the
/// database from computing one anyway.
const UNIT_CODE_COLUMN: &str = r#""ah"."unit_code_id""#;

/// The value kind an aggregated group's observations were recorded under.
///
/// Grouped as well as projected, for the same reason as the attribute kind:
/// `min` and `max` are taken from the typed column the kind names, so a group
/// spanning two value kinds would have to pick one of two equally valid
/// extrema. Grouping on the kind makes each group's extremum unambiguous and
/// lets the reader select the column the values actually live in.
const VALUE_KIND_COLUMN: &str = r#""ah"."value_kind""#;

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
            format!("\"an\".\"name\", bucket_start, {UNIT_CODE_COLUMN}"),
        )
    } else {
        (
            "NULL::timestamptz AS bucket_start, NULL::timestamptz AS bucket_end".to_string(),
            format!("\"an\".\"name\", {UNIT_CODE_COLUMN}"),
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
        format!("\"an\".\"name\", {ATTRIBUTE_KIND_COLUMN}, {VALUE_KIND_COLUMN}, \"s\".\"dataset_id\", {UNIT_CODE_COLUMN}, bucket_start")
    } else {
        format!("\"an\".\"name\", {ATTRIBUTE_KIND_COLUMN}, {VALUE_KIND_COLUMN}, \"s\".\"dataset_id\", {UNIT_CODE_COLUMN}")
    };

    let sql = format!(
        r#"SELECT
    "an"."name" AS attribute_name,
    {ATTRIBUTE_KIND_COLUMN} AS attribute_kind,
    {VALUE_KIND_COLUMN} AS value_kind,
    "s"."dataset_id",
    {UNIT_CODE_COLUMN} AS unit_code_id,
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

/// Whether an aggregate's result depends on the unit of the values it read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnitSensitivity {
    /// The result is itself a quantity, expressed in the unit of its inputs.
    Sensitive,
    /// The result counts instances, so which unit each one carries is irrelevant.
    Insensitive,
}

/// Classify an aggregation method of clause 4.5.19 by whether its result carries a unit.
pub const fn unit_sensitivity(method: AggrMethod) -> UnitSensitivity {
    match method {
        AggrMethod::Avg | AggrMethod::Min | AggrMethod::Max | AggrMethod::Sum | AggrMethod::SumSq | AggrMethod::StdDev => UnitSensitivity::Sensitive,
        AggrMethod::TotalCount | AggrMethod::DistinctCount => UnitSensitivity::Insensitive,
    }
}

/// The key one aggregate is reported under, with the unit deliberately left out.
///
/// The query groups on the unit as well, so two groups that share a target and
/// differ in unit are exactly the case clause 4.5.19 leaves undefined.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AggregationTarget {
    /// Expanded name of the aggregated attribute.
    pub attribute_name: AttributeName,
    /// `datasetId` of the aggregated instances (clause 4.5.4), absent for the default instance.
    pub dataset_id: Option<Uri>,
    /// Start of the `time_bucket` window, absent when the whole range is one bucket.
    pub bucket_start: Option<DateTime<Utc>>,
}

/// A unit-sensitive aggregate would have had to span more than one unit.
///
/// Recommendation 20 carries conversion factors only informatively and NGSI-LD
/// asks no broker to apply them, so the aggregate is refused rather than
/// computed over values that do not share a scale.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MixedUnitAggregation {
    /// Two different unit codes were recorded for one aggregation target.
    #[error("attribute '{attribute_name}' aggregates over unit codes {first} and {second}, which the broker does not convert between")]
    ConflictingUnits {
        /// Expanded name of the aggregated attribute.
        attribute_name: AttributeName,
        /// The unit first observed for the target.
        first: UnitCodeId,
        /// The unit that contradicted it.
        second: UnitCodeId,
    },
    /// Values with a unit and values without one were recorded for one aggregation target.
    #[error("attribute '{attribute_name}' aggregates over values carrying unit code {present} and values carrying no unit at all")]
    UnitAndUnitless {
        /// Expanded name of the aggregated attribute.
        attribute_name: AttributeName,
        /// The unit the annotated values carried.
        present: UnitCodeId,
    },
}

/// Refuse an aggregated result set whose unit-sensitive aggregates span several units.
///
/// `groups` is the query's output keyed as it was grouped: one entry per
/// target and unit. Methods that only count are unaffected, so a request made
/// up entirely of `totalCount` and `distinctCount` — including the empty
/// method list, which counts by default — is answered over mixed units
/// unchanged.
pub fn ensure_single_unit_per_target<'a, Groups>(groups: Groups, methods: &[AggrMethod]) -> Result<(), MixedUnitAggregation>
where
    Groups: IntoIterator<Item = (&'a AggregationTarget, Option<UnitCodeId>)>,
{
    if !methods
        .iter()
        .copied()
        .map(unit_sensitivity)
        .any(|sensitivity| sensitivity == UnitSensitivity::Sensitive)
    {
        return Ok(());
    }

    let mut seen: HashMap<&AggregationTarget, Option<UnitCodeId>> = HashMap::new();
    for (target, unit) in groups {
        match seen.entry(target) {
            Entry::Vacant(slot) => {
                slot.insert(unit);
            }
            // The error names the attribute, so it owns a copy of a name the
            // caller keeps: the borrow ends with the result set.
            Entry::Occupied(slot) => match (*slot.get(), unit) {
                (Some(first), Some(second)) if first != second => {
                    return Err(MixedUnitAggregation::ConflictingUnits {
                        attribute_name: target.attribute_name.clone(),
                        first,
                        second,
                    });
                }
                (Some(present), None) | (None, Some(present)) => {
                    return Err(MixedUnitAggregation::UnitAndUnitless {
                        attribute_name: target.attribute_name.clone(),
                        present,
                    });
                }
                (Some(_), Some(_)) | (None, None) => {}
            },
        }
    }

    Ok(())
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

    /// The kind is a fact of the row, so it is grouped and rendered from the
    /// hypertable rather than read back from the stream catalogue.
    #[test]
    fn the_aggregated_group_carries_the_attribute_kind_of_its_rows() {
        let compiled = aggregated(vec![AggrMethod::Avg], Some("PT1H"));
        let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
        assert!(built.sql.contains("\"ah\".\"attribute_kind\" AS attribute_kind"), "{}", built.sql);
        let group_by = built.sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
        assert!(group_by.contains("\"ah\".\"attribute_kind\""), "{}", built.sql);
        assert!(!built.sql.contains("attribute_type"), "{}", built.sql);
        assert!(!built.sql.contains("value_type"), "{}", built.sql);
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

    /// Clause 4.5.2 attaches a `unitCode` to the instance, not to the
    /// attribute, so two instances of one attribute may differ in unit. The
    /// aggregate is therefore computed per unit rather than across units.
    #[test]
    fn every_aggregate_is_grouped_by_the_unit_its_values_were_recorded_in() {
        for period in [Some("PT1H"), None] {
            let compiled = aggregated(vec![AggrMethod::Avg], period);
            let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
            assert!(built.sql.contains("\"ah\".\"unit_code_id\" AS unit_code_id"), "{}", built.sql);
            let group_by = built.sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
            assert!(group_by.contains("\"ah\".\"unit_code_id\""), "{}", built.sql);
        }
    }

    fn target(name: &str) -> AggregationTarget {
        AggregationTarget {
            attribute_name: name.parse::<AttributeName>().expect("valid attribute name"),
            dataset_id: None,
            bucket_start: None,
        }
    }

    fn unit(id: i16) -> Option<UnitCodeId> {
        Some(UnitCodeId::try_from(id).expect("a registered unit identity"))
    }

    #[test]
    fn an_aggregate_over_one_unit_is_accepted_unchanged() {
        let temperature = target("https://uri.etsi.org/ngsi-ld/default-context/temperature");
        let groups = vec![(&temperature, unit(367)), (&temperature, unit(367))];
        pretty_assertions::assert_eq!(ensure_single_unit_per_target(groups, &[AggrMethod::Avg]), Ok(()));
    }

    #[test]
    fn an_aggregate_spanning_two_units_is_refused() {
        let name = "https://uri.etsi.org/ngsi-ld/default-context/weight";
        let weight = target(name);
        let groups = vec![(&weight, unit(1121)), (&weight, unit(1407))];
        let expected = MixedUnitAggregation::ConflictingUnits {
            attribute_name: name.parse::<AttributeName>().expect("valid attribute name"),
            first: UnitCodeId::try_from(1121_i16).expect("a registered unit identity"),
            second: UnitCodeId::try_from(1407_i16).expect("a registered unit identity"),
        };
        pretty_assertions::assert_eq!(ensure_single_unit_per_target(groups, &[AggrMethod::Avg]), Err(expected));
    }

    #[test]
    fn an_aggregate_mixing_annotated_and_unannotated_values_is_refused() {
        let name = "https://uri.etsi.org/ngsi-ld/default-context/weight";
        let weight = target(name);
        let groups = vec![(&weight, unit(1121)), (&weight, None)];
        let expected = MixedUnitAggregation::UnitAndUnitless {
            attribute_name: name.parse::<AttributeName>().expect("valid attribute name"),
            present: UnitCodeId::try_from(1121_i16).expect("a registered unit identity"),
        };
        pretty_assertions::assert_eq!(ensure_single_unit_per_target(groups, &[AggrMethod::Sum]), Err(expected));
    }

    #[test]
    fn an_attribute_that_carries_no_unit_aggregates_normally() {
        let occupancy = target("https://uri.etsi.org/ngsi-ld/default-context/occupancy");
        let groups = vec![(&occupancy, None), (&occupancy, None)];
        pretty_assertions::assert_eq!(ensure_single_unit_per_target(groups, &[AggrMethod::Avg, AggrMethod::StdDev]), Ok(()));
    }

    /// Counting instances says nothing about the scale they were measured on,
    /// so a count over mixed units is well defined and stays answerable.
    #[test]
    fn counting_aggregates_survive_mixed_units() {
        let weight = target("https://uri.etsi.org/ngsi-ld/default-context/weight");
        let groups = vec![(&weight, unit(1121)), (&weight, unit(1407))];
        pretty_assertions::assert_eq!(
            ensure_single_unit_per_target(groups.clone(), &[AggrMethod::TotalCount, AggrMethod::DistinctCount]),
            Ok(())
        );
        pretty_assertions::assert_eq!(ensure_single_unit_per_target(groups, &[]), Ok(()));
    }

    #[test]
    fn two_targets_may_each_hold_their_own_unit() {
        let weight = target("https://uri.etsi.org/ngsi-ld/default-context/weight");
        let length = target("https://uri.etsi.org/ngsi-ld/default-context/length");
        let groups = vec![(&weight, unit(1121)), (&length, unit(1407))];
        pretty_assertions::assert_eq!(ensure_single_unit_per_target(groups, &[AggrMethod::Max]), Ok(()));
    }

    #[test]
    fn every_aggregation_method_is_classified_as_carrying_a_unit_or_not() {
        for method in [
            AggrMethod::Avg,
            AggrMethod::Min,
            AggrMethod::Max,
            AggrMethod::Sum,
            AggrMethod::SumSq,
            AggrMethod::StdDev,
        ] {
            pretty_assertions::assert_eq!(unit_sensitivity(method), UnitSensitivity::Sensitive, "{method}");
        }
        for method in [AggrMethod::TotalCount, AggrMethod::DistinctCount] {
            pretty_assertions::assert_eq!(unit_sensitivity(method), UnitSensitivity::Insensitive, "{method}");
        }
    }

    /// The reader picks the typed `min`/`max` column from the value kind, so
    /// the kind has to survive the aggregation as a projected group column.
    #[test]
    fn the_aggregated_group_carries_the_value_kind_its_extrema_are_typed_by() {
        for period in [Some("PT1H"), None] {
            let compiled = aggregated(vec![AggrMethod::Min, AggrMethod::Max], period);
            let built = build_aggregation_query_sql(&compiled, "\"te\".\"id\" = $1", BindBuilder::default()).unwrap();
            assert!(built.sql.contains("\"ah\".\"value_kind\" AS value_kind"), "{}", built.sql);
            let group_by = built.sql.split("GROUP BY").nth(1).expect("a GROUP BY clause");
            assert!(group_by.contains("\"ah\".\"value_kind\""), "{}", built.sql);
        }
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
