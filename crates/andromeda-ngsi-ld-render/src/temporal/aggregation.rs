//! Temporal aggregation: [`TemporalAggregationRenderer`] groups decoded
//! aggregation rows by `(attribute_name, dataset_id)` and extracts
//! aggregation method values (NGSI-LD aggregation methods: avg, min, max,
//! sum, sumsq, stddev, totalCount, distinctCount).
//!
//! Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
//! period, and the store returns exactly that: one row per period of one
//! attribute instance, in period order. Each row therefore becomes exactly one
//! `[value, start, end]` triple, and two triples of one instance can never
//! carry the same period bounds.

use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    rfc3339::sub_second,
};
use andromeda_ngsi_ld_model::temporal::{AggregationAverage, AggregationMinMax, GroupKeyValue, TemporalAggregationRow};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use indexmap::IndexMap;
use serde_json::{Value, json};

/// The key one attribute instance is grouped under: its name plus the
/// `datasetId` that clause 4.5.5 uses to tell the instances of one attribute
/// apart.
type GroupKey = (AttributeName, Option<Identifier>);

/// Builds aggregated temporal data from decoded aggregation rows.
///
/// Groups rows by `(attribute_name, dataset_id)` and extracts aggregation
/// method values (avg, min, max, sum, etc.) with their bucket time ranges.
/// Returns an ordered map of attribute name to aggregated instance(s).
#[derive(Debug)]
pub struct TemporalAggregationRenderer<'a> {
    /// Neutral aggregation buckets to project.
    rows: &'a [TemporalAggregationRow],
    /// Compiled representation and requested aggregation methods.
    compiled: &'a CompiledTemporalQuery,
}

impl<'a> TemporalAggregationRenderer<'a> {
    /// Create a renderer for the given row set.
    #[must_use]
    pub const fn new(rows: &'a [TemporalAggregationRow], compiled: &'a CompiledTemporalQuery) -> TemporalAggregationRenderer<'a> {
        TemporalAggregationRenderer { rows, compiled }
    }
}

/// Render an [`AggregationMinMax`] as a JSON value.
///
/// The numeric extremum serialises through its own JSON-number shape, so an
/// integer beyond 2^53 is emitted as that integer rather than as the double
/// nearest to it.
fn aggregation_average_to_json(value: Option<&AggregationAverage>) -> Value {
    match value {
        Some(AggregationAverage::Number(number)) => json!(number),
        // Table 4.5.19.1-2: `avg` over a `Time` is "the average time inside the
        // period", so it is rendered in the clause 4.6.3 `Time` lexical form
        // rather than as the number of seconds it was computed from.
        Some(AggregationAverage::Time(time)) => json!(time.to_string()),
        None => Value::Null,
    }
}

/// The clause 4.5.19 member name one aggregation method renders under.
const fn aggregation_method_key(method: AggrMethod) -> &'static str {
    match method {
        AggrMethod::Avg => "avg",
        AggrMethod::Min => "min",
        AggrMethod::Max => "max",
        AggrMethod::Sum => "sum",
        AggrMethod::SumSq => "sumsq",
        AggrMethod::StdDev => "stddev",
        AggrMethod::TotalCount => "totalCount",
        AggrMethod::DistinctCount => "distinctCount",
    }
}

/// The `[value, start, end]` triples one method contributes over a set of rows.
fn method_triples(rows: &[&TemporalAggregationRow], method: AggrMethod) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            let value: Value = match method {
                AggrMethod::TotalCount => row.total_count.map_or(Value::Null, |count| json!(count)),
                AggrMethod::DistinctCount => row.distinct_count.map_or(Value::Null, |count| json!(count)),
                AggrMethod::Min => aggregation_min_max_to_json(row.min.as_ref()),
                AggrMethod::Max => aggregation_min_max_to_json(row.max.as_ref()),
                AggrMethod::Avg => aggregation_average_to_json(row.avg.as_ref()),
                AggrMethod::Sum => row.sum.as_ref().map_or(Value::Null, |number| json!(number)),
                AggrMethod::SumSq => row.sumsq.as_ref().map_or(Value::Null, |number| json!(number)),
                AggrMethod::StdDev => row.stddev.as_ref().map_or(Value::Null, |number| json!(number)),
            };
            // Bucket bounds are derived from the client's `timeAt` and
            // `aggrPeriodDuration`, so a `timeAt` carrying milliseconds must
            // render them, or the period a triple names is mislabelled.
            let bucket_start = row.bucket_start.map_or(Value::Null, |instant| json!(sub_second(instant)));
            let bucket_end = row.bucket_end.map_or(Value::Null, |instant| json!(sub_second(instant)));
            json!([value, bucket_start, bucket_end])
        })
        .collect()
}

/// Nest one method's triples under one map key per grouping term (study §4.6).
///
/// At each level the rows are partitioned by their group-key value at that
/// level, preserving the order the store returned them in (clause 4.23.2 plus
/// the query's collation, applied by the SQL `ORDER BY`). The leaf, once every
/// term has been consumed, is the triple array the standard format would emit.
fn nested_group_value(rows: &[&TemporalAggregationRow], method: AggrMethod, level: usize, depth: usize) -> Value {
    if level == depth {
        return Value::Array(method_triples(rows, method));
    }
    let mut buckets: IndexMap<String, Vec<&TemporalAggregationRow>> = IndexMap::new();
    for row in rows {
        let key = row.group_keys.get(level).map_or(GroupKeyValue::NONE_KEY, GroupKeyValue::as_map_key).to_owned();
        buckets.entry(key).or_default().push(*row);
    }
    let mut map = serde_json::Map::with_capacity(buckets.len());
    for (key, sub_rows) in buckets {
        map.insert(key, nested_group_value(&sub_rows, method, level + 1, depth));
    }
    Value::Object(map)
}

/// Render a typed minimum or maximum in the family it was observed in.
fn aggregation_min_max_to_json(value: Option<&AggregationMinMax>) -> Value {
    match value {
        Some(AggregationMinMax::Number(number)) => json!(number),
        Some(AggregationMinMax::String(text)) => json!(text),
        Some(AggregationMinMax::Boolean(flag)) => json!(flag),
        // A DateTime extremum is a client-supplied value, so it keeps the
        // sub-second precision it was observed with.
        Some(AggregationMinMax::DateTime(instant)) => json!(sub_second(*instant)),
        None => Value::Null,
    }
}

impl TemporalAggregationRenderer<'_> {
    /// Group the rows and extract each requested aggregation method.
    #[must_use]
    pub fn render(self) -> IndexMap<AttributeName, Value> {
        let TemporalAggregationRenderer { rows, compiled } = self;

        // The family is carried on the group rather than re-read from a row,
        // which is what removes the "no rows in this group" case an index built
        // by pushing can never actually produce.
        let mut groups: IndexMap<GroupKey, (AttributeType, Vec<&TemporalAggregationRow>)> = IndexMap::with_capacity(rows.len().min(64));

        for row in rows {
            groups
                // clone: the index owns the key its borrowed rows are filed under.
                .entry((row.attribute_name.clone(), row.dataset_id.clone()))
                .or_insert_with(|| (row.attribute_type, Vec::new()))
                .1
                .push(row);
        }

        let mut attrs: IndexMap<AttributeName, Vec<Value>> = IndexMap::with_capacity(groups.len());

        for ((attr_name, dataset_id), (attribute_type, bucket_rows)) in &groups {
            let mut instance = serde_json::Map::with_capacity(2 + compiled.representation.aggr_methods.len());
            instance.insert("type".to_string(), json!(attribute_type));

            if let Some(dataset_id) = dataset_id {
                instance.insert("datasetId".to_string(), json!(dataset_id));
            }

            // The grouped extension format nests the triples under one map key
            // per grouping term, between the method member and its array (study
            // §4.6); the standard format emits the array directly.
            let grouping_depth = if compiled.representation.format.is_grouped() {
                compiled.representation.grouping.as_ref().map_or(0, |grouping| grouping.terms().len())
            } else {
                0
            };
            for method in &compiled.representation.aggr_methods {
                let key = aggregation_method_key(*method);
                let value = if grouping_depth == 0 {
                    Value::Array(method_triples(bucket_rows, *method))
                } else {
                    nested_group_value(bucket_rows, *method, 0, grouping_depth)
                };
                instance.insert(key.to_string(), value);
            }

            // clone: the outer index owns the attribute key its instances hang off.
            attrs.entry(attr_name.clone()).or_default().push(Value::Object(instance));
        }

        let mut result: IndexMap<AttributeName, Value> = IndexMap::with_capacity(attrs.len());
        for (attr_name, instances) in attrs {
            // Clause 4.5.4: one instance renders as an object and several as an
            // array. The array conversion is what makes the single-instance
            // case total, rather than a branch that could silently drop it.
            let rendered = match <[Value; 1]>::try_from(instances) {
                Ok([single]) => single,
                Err(several) => Value::Array(several),
            };
            result.insert(attr_name, rendered);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::aggregation::TemporalAggregationRenderer;
    use andromeda_ngsi_ld_lexicon::{
        aggr_method::AggrMethod,
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        temporal_format::TemporalFormat,
        time_prop::TimeProp,
    };
    use andromeda_ngsi_ld_model::temporal::{AggregationAverage, AggregationMinMax, GroupKeyValue, TemporalAggregationRow};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{compiled::CompiledTemporalQuery, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};
    use std::str::FromStr;

    const WEIGHT: &str = "https://uri.etsi.org/ngsi-ld/default-context/weight";

    fn requesting(methods: Vec<AggrMethod>) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::AggregatedValues)
                    .aggr_methods(methods)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    /// One period of one attribute instance, with every aggregate absent.
    fn bucket(bucket_start: Option<&str>, bucket_end: Option<&str>) -> TemporalAggregationRow {
        TemporalAggregationRow {
            attribute_name: WEIGHT.parse::<AttributeName>().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            bucket_start: bucket_start.map(instant),
            bucket_end: bucket_end.map(instant),
            avg: None,
            min: None,
            max: None,
            sum: None,
            sumsq: None,
            stddev: None,
            total_count: None,
            distinct_count: None,
            group_keys: Vec::new(),
        }
    }

    fn rendered(rows: &[TemporalAggregationRow], methods: Vec<AggrMethod>) -> Value {
        let compiled = requesting(methods);
        let attributes = TemporalAggregationRenderer::new(rows, &compiled).render();
        let name = WEIGHT.parse::<AttributeName>().expect("valid attribute name");
        attributes.get(&name).cloned().expect("the aggregated attribute is rendered")
    }

    /// An exact decimal stays exact all the way to the wire: an integer beyond
    /// 2^53 is emitted as itself rather than as the double nearest to it
    /// (`docs/exact-numeric-representation.md`).
    #[test]
    fn an_exact_sum_is_rendered_as_the_integer_it_is() {
        let mut row = bucket(None, None);
        row.sum = Some(JsonNumber::from(9_007_199_254_740_993_i64));
        let instance = rendered(&[row], vec![AggrMethod::Sum]);
        pretty_assertions::assert_eq!(instance["sum"], json!([[9_007_199_254_740_993_i64, Value::Null, Value::Null]]));
        assert!(
            serde_json::to_string(&instance).expect("the instance serialises").contains("9007199254740993"),
            "{instance}"
        );
    }

    #[test]
    fn every_decimal_aggregate_is_rendered_without_binary64_conversion() {
        let exact = |value: &str| JsonNumber::from_str(value).expect("supported exact aggregate");
        let mut row = bucket(None, None);
        row.avg = Some(AggregationAverage::Number(exact("0.150000000000000005")));
        row.min = Some(AggregationMinMax::Number(exact("0.100000000000000005")));
        row.max = Some(AggregationMinMax::Number(exact("0.200000000000000005")));
        row.sum = Some(exact("0.300000000000000010"));
        row.sumsq = Some(exact("0.050000000000000003000000000000000050"));
        row.stddev = Some(exact("0.070710678118654752"));
        let instance = rendered(
            &[row],
            vec![
                AggrMethod::Avg,
                AggrMethod::Min,
                AggrMethod::Max,
                AggrMethod::Sum,
                AggrMethod::SumSq,
                AggrMethod::StdDev,
            ],
        );
        let encoded = serde_json::to_string(&instance).expect("aggregate JSON");
        for expected in [
            "0.150000000000000005",
            "0.100000000000000005",
            "0.200000000000000005",
            "0.30000000000000001",
            "0.05000000000000000300000000000000005",
            "0.070710678118654752",
        ] {
            assert!(encoded.contains(expected), "missing {expected} in {encoded}");
        }
    }

    /// Clause 4.5.19 reports the extremum of the values that were observed, and
    /// a boolean attribute observed `false` and `true` — not the zero and one
    /// the backend counted them as.
    #[test]
    fn a_boolean_extremum_is_rendered_in_the_family_it_was_observed_in() {
        let mut row = bucket(None, None);
        row.min = Some(AggregationMinMax::Boolean(false));
        row.max = Some(AggregationMinMax::Boolean(true));
        let instance = rendered(&[row], vec![AggrMethod::Min, AggrMethod::Max]);
        pretty_assertions::assert_eq!(instance["min"], json!([[false, Value::Null, Value::Null]]));
        pretty_assertions::assert_eq!(instance["max"], json!([[true, Value::Null, Value::Null]]));
    }

    /// Every [`AggregationMinMax`] member renders in its own JSON shape, so a
    /// string extremum never arrives as a number and an instant never as an
    /// unformatted timestamp.
    #[test]
    fn each_extremum_family_renders_in_its_own_json_shape() {
        let cases = [
            (AggregationMinMax::Number(JsonNumber::try_from(1.5).expect("a finite number")), json!(1.5)),
            (AggregationMinMax::String("alpha".to_owned()), json!("alpha")),
            (AggregationMinMax::Boolean(true), json!(true)),
            (AggregationMinMax::DateTime(instant("2026-01-01T00:00:00Z")), json!("2026-01-01T00:00:00Z")),
        ];
        for (extremum, expected) in cases {
            let mut row = bucket(None, None);
            row.min = Some(extremum.clone()); // clone: the case table is reused for the failure message.
            let instance = rendered(&[row], vec![AggrMethod::Min]);
            pretty_assertions::assert_eq!(instance["min"], json!([[expected, Value::Null, Value::Null]]), "{extremum:?}");
        }
    }

    /// A method the row carries no value for renders as JSON null rather than
    /// disappearing from the triple, so the three members always line up.
    #[test]
    fn an_absent_aggregate_renders_as_a_null_member_of_its_triple() {
        let instance = rendered(&[bucket(Some("2026-01-01T00:00:00Z"), Some("2026-01-01T01:00:00Z"))], vec![AggrMethod::Avg]);
        pretty_assertions::assert_eq!(instance["avg"], json!([[Value::Null, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z"]]));
    }

    /// Clause 4.5.19: one aggregate value per period. The store returns one row
    /// per period, so two periods produce two triples with distinct bounds and
    /// never two triples repeating one period.
    #[test]
    fn each_period_contributes_exactly_one_triple() {
        let mut first = bucket(Some("2026-01-01T00:00:00Z"), Some("2026-01-01T01:00:00Z"));
        first.total_count = Some(2);
        let mut second = bucket(Some("2026-01-01T01:00:00Z"), Some("2026-01-01T02:00:00Z"));
        second.total_count = Some(3);
        let instance = rendered(&[first, second], vec![AggrMethod::TotalCount]);
        pretty_assertions::assert_eq!(
            instance["totalCount"],
            json!([
                [2, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z"],
                [3, "2026-01-01T01:00:00Z", "2026-01-01T02:00:00Z"]
            ])
        );
        pretty_assertions::assert_eq!(instance["type"], json!("Property"));
    }

    fn grouped_requesting(methods: Vec<AggrMethod>, aggr_group_by: &str) -> CompiledTemporalQuery {
        use andromeda_ngsi_ld_query::temporal::grouping::AggregationGrouping;
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::GroupedValues)
                    .aggr_methods(methods)
                    .grouping(Some(AggregationGrouping::parse(aggr_group_by).expect("a well-formed grouping")))
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn grouped_bucket(count: i64, keys: Vec<GroupKeyValue>) -> TemporalAggregationRow {
        let mut row = bucket(None, None);
        row.total_count = Some(count);
        row.group_keys = keys;
        row
    }

    fn grouped_instance(rows: &[TemporalAggregationRow], compiled: &CompiledTemporalQuery) -> Value {
        let attributes = TemporalAggregationRenderer::new(rows, compiled).render();
        let name = WEIGHT.parse::<AttributeName>().expect("valid attribute name");
        attributes.get(&name).cloned().expect("the aggregated attribute is rendered")
    }

    /// Study §4.6: one grouping term nests a map keyed by the group value between
    /// the method member and its triple array.
    #[test]
    fn one_grouping_term_nests_a_map_keyed_by_the_group_value() {
        let compiled = grouped_requesting(vec![AggrMethod::TotalCount], "unitCode");
        let rows = [
            grouped_bucket(2, vec![GroupKeyValue::Value("KGM".to_owned())]),
            grouped_bucket(3, vec![GroupKeyValue::Value("LBR".to_owned())]),
        ];
        let instance = grouped_instance(&rows, &compiled);
        pretty_assertions::assert_eq!(instance["totalCount"]["KGM"], json!([[2, Value::Null, Value::Null]]));
        pretty_assertions::assert_eq!(instance["totalCount"]["LBR"], json!([[3, Value::Null, Value::Null]]));
    }

    /// Two terms nest two levels, in the order the terms were written.
    #[test]
    fn two_grouping_terms_nest_two_levels_in_order() {
        let compiled = grouped_requesting(vec![AggrMethod::TotalCount], "unitCode,observedAt;day");
        let rows = [grouped_bucket(
            5,
            vec![GroupKeyValue::Value("KGM".to_owned()), GroupKeyValue::Value("2026-01-01T00:00:00Z".to_owned())],
        )];
        let instance = grouped_instance(&rows, &compiled);
        pretty_assertions::assert_eq!(instance["totalCount"]["KGM"]["2026-01-01T00:00:00Z"], json!([[5, Value::Null, Value::Null]]));
    }

    /// Study §4.6: an absent key renders as `@none` and an explicit NGSI-LD Null
    /// as `urn:ngsi-ld:null`.
    #[test]
    fn absent_and_null_keys_use_their_reserved_map_keys() {
        let compiled = grouped_requesting(vec![AggrMethod::TotalCount], "category");
        let rows = [grouped_bucket(1, vec![GroupKeyValue::Absent]), grouped_bucket(2, vec![GroupKeyValue::Null])];
        let instance = grouped_instance(&rows, &compiled);
        pretty_assertions::assert_eq!(instance["totalCount"]["@none"], json!([[1, Value::Null, Value::Null]]));
        pretty_assertions::assert_eq!(instance["totalCount"]["urn:ngsi-ld:null"], json!([[2, Value::Null, Value::Null]]));
    }

    /// A single group degenerates to a map with one entry, and the ungrouped
    /// `aggregatedValues` output is unchanged (regression).
    #[test]
    fn a_single_group_is_one_map_entry_and_ungrouped_output_is_unchanged() {
        let compiled = grouped_requesting(vec![AggrMethod::TotalCount], "unitCode");
        let rows = [grouped_bucket(7, vec![GroupKeyValue::Value("KGM".to_owned())])];
        let instance = grouped_instance(&rows, &compiled);
        pretty_assertions::assert_eq!(instance["totalCount"].as_object().expect("a group map").len(), 1);

        // The ungrouped format still emits the bare triple array.
        let mut plain = bucket(None, None);
        plain.total_count = Some(7);
        let ungrouped = rendered(&[plain], vec![AggrMethod::TotalCount]);
        pretty_assertions::assert_eq!(ungrouped["totalCount"], json!([[7, Value::Null, Value::Null]]));
    }

    /// Clause 4.5.4: instances of one attribute are told apart by `datasetId`,
    /// so two of them render as an array of two objects rather than as one.
    #[test]
    fn two_dataset_instances_render_as_an_array_of_instances() {
        let mut default_instance = bucket(None, None);
        default_instance.total_count = Some(1);
        let mut named_instance = bucket(None, None);
        named_instance.dataset_id = Some("urn:ngsi-ld:Dataset:one".parse().expect("valid dataset id"));
        named_instance.total_count = Some(4);
        let instance = rendered(&[default_instance, named_instance], vec![AggrMethod::TotalCount]);
        let instances = instance.as_array().expect("two instances render as an array");
        pretty_assertions::assert_eq!(instances.len(), 2, "{instance}");
        pretty_assertions::assert_eq!(instances[1]["datasetId"], json!("urn:ngsi-ld:Dataset:one"));
    }

    /// A `DateTime` extremum is a client-supplied value, so it keeps the
    /// sub-second precision it was observed with and agrees with the temporal
    /// history (clauses 5.7.3/5.7.4, 4.6.3).
    #[test]
    fn a_datetime_extremum_keeps_its_milliseconds() {
        let mut row = bucket(None, None);
        row.min = Some(AggregationMinMax::DateTime(instant("2026-01-01T08:39:59.810Z")));
        row.max = Some(AggregationMinMax::DateTime(instant("2026-01-01T08:40:01.250Z")));
        let instance = rendered(&[row], vec![AggrMethod::Min, AggrMethod::Max]);
        pretty_assertions::assert_eq!(instance["min"], json!([["2026-01-01T08:39:59.810Z", Value::Null, Value::Null]]));
        pretty_assertions::assert_eq!(instance["max"], json!([["2026-01-01T08:40:01.250Z", Value::Null, Value::Null]]));
    }

    /// Bucket bounds derived from a `timeAt` carrying milliseconds render those
    /// milliseconds, so the period a triple names is the one that was actually
    /// aggregated rather than a whole-second window it never spanned.
    #[test]
    fn bucket_bounds_keep_the_sub_second_precision_of_the_window() {
        let row = bucket(Some("2026-01-01T00:00:00.050Z"), Some("2026-01-01T00:00:00.200Z"));
        let instance = rendered(&[row], vec![AggrMethod::TotalCount]);
        pretty_assertions::assert_eq!(
            instance["totalCount"],
            json!([[Value::Null, "2026-01-01T00:00:00.050Z", "2026-01-01T00:00:00.200Z"]])
        );
    }
}
