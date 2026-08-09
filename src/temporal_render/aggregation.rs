//! Temporal aggregation: [`TemporalAggregationRenderer`] groups decoded
//! aggregation rows by `(attribute_name, dataset_id)` and extracts
//! aggregation method values (NGSI-LD aggregation methods: avg, min, max,
//! sum, sumsq, stddev, totalCount, distinctCount).

use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, rfc3339::whole_seconds, uri::Uri};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_temporal_store::store::{AggregationMinMax, TemporalAggregationRow};
use indexmap::IndexMap;
use serde_json::{Value, json};

/// Builds aggregated temporal data from decoded aggregation rows.
///
/// Groups rows by `(attribute_name, dataset_id)` and extracts aggregation
/// method values (avg, min, max, sum, etc.) with their bucket time ranges.
/// Returns an ordered map of attribute name to aggregated instance(s).
#[derive(Debug)]
pub struct TemporalAggregationRenderer<'a> {
    rows: &'a [TemporalAggregationRow],
    compiled: &'a CompiledTemporalQuery,
}

impl<'a> TemporalAggregationRenderer<'a> {
    /// Create a renderer for the given row set.
    pub const fn new(rows: &'a [TemporalAggregationRow], compiled: &'a CompiledTemporalQuery) -> TemporalAggregationRenderer<'a> {
        TemporalAggregationRenderer { rows, compiled }
    }
}

/// Render an [`AggregationMinMax`] as a JSON value.
fn aggregation_min_max_to_json(value: &Option<AggregationMinMax>) -> Value {
    match value {
        Some(AggregationMinMax::Number(v)) => json!(v),
        Some(AggregationMinMax::String(v)) => json!(v),
        Some(AggregationMinMax::DateTime(dt)) => json!(whole_seconds(*dt)),
        None => Value::Null,
    }
}

impl<'a> TemporalAggregationRenderer<'a> {
    /// Group the rows and extract each requested aggregation method.
    pub fn render(self) -> IndexMap<AttributeName, Value> {
        let TemporalAggregationRenderer { rows, compiled } = self;

        type GroupKey = (AttributeName, Option<Uri>);
        let mut groups: IndexMap<GroupKey, Vec<&TemporalAggregationRow>> = IndexMap::with_capacity(rows.len().min(64));

        for row in rows {
            groups.entry((row.attribute_name.clone(), row.dataset_id.clone())).or_default().push(row);
        }

        let mut attrs: IndexMap<AttributeName, Vec<Value>> = IndexMap::with_capacity(groups.len());

        for ((attr_name, dataset_id), bucket_rows) in &groups {
            let first_row = match bucket_rows.first() {
                Some(r) => r,
                None => continue,
            };

            let mut instance = serde_json::Map::with_capacity(2 + compiled.representation.aggr_methods.len());
            instance.insert("type".to_string(), json!(&first_row.attribute_type));

            if let Some(ds_id) = dataset_id {
                instance.insert("datasetId".to_string(), json!(ds_id));
            }

            for method in &compiled.representation.aggr_methods {
                let key = match method {
                    AggrMethod::Avg => "avg",
                    AggrMethod::Min => "min",
                    AggrMethod::Max => "max",
                    AggrMethod::Sum => "sum",
                    AggrMethod::SumSq => "sumsq",
                    AggrMethod::StdDev => "stddev",
                    AggrMethod::TotalCount => "totalCount",
                    AggrMethod::DistinctCount => "distinctCount",
                };

                let mut triples: Vec<Value> = Vec::with_capacity(bucket_rows.len());
                for row in bucket_rows {
                    let val: Value = match method {
                        AggrMethod::TotalCount => row.total_count.map_or(Value::Null, |v| json!(v)),
                        AggrMethod::DistinctCount => row.distinct_count.map_or(Value::Null, |v| json!(v)),
                        AggrMethod::Min => aggregation_min_max_to_json(&row.min),
                        AggrMethod::Max => aggregation_min_max_to_json(&row.max),
                        AggrMethod::Avg => row.avg.map_or(Value::Null, |v| json!(v)),
                        AggrMethod::Sum => row.sum.map_or(Value::Null, |v| json!(v)),
                        AggrMethod::SumSq => row.sumsq.map_or(Value::Null, |v| json!(v)),
                        AggrMethod::StdDev => row.stddev.map_or(Value::Null, |v| json!(v)),
                    };

                    let bucket_start = row.bucket_start.map_or(Value::Null, |dt| json!(whole_seconds(dt)));
                    let bucket_end = row.bucket_end.map_or(Value::Null, |dt| json!(whole_seconds(dt)));

                    triples.push(json!([val, bucket_start, bucket_end]));
                }

                instance.insert(key.to_string(), Value::Array(triples));
            }

            attrs.entry(attr_name.clone()).or_default().push(Value::Object(instance));
        }

        let mut result: IndexMap<AttributeName, Value> = IndexMap::with_capacity(attrs.len());
        for (attr_name, instances) in attrs {
            if instances.len() == 1 {
                if let Some(single) = instances.into_iter().next() {
                    result.insert(attr_name, single);
                }
            } else {
                result.insert(attr_name, Value::Array(instances));
            }
        }

        result
    }
}
