//! Temporal aggregation: [`TemporalAggregationRenderer`] groups database rows
//! by `(attribute_name, dataset_id)` and extracts aggregation method values
//! (NGSI-LD aggregation methods: avg, min, max, sum, sumsq, stddev,
//! totalCount, distinctCount).

use crate::{
    core::error::AndromedaError,
    renderer::{Renderer, temporal::common::extract_typed_agg},
    service::temporal::format_datetime,
};
use andromeda_domain::temporal::{AggrMethod, ValueType};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;

/// Builds aggregated temporal data from database rows.
///
/// Groups rows by `(attribute_name, dataset_id)` and extracts aggregation
/// method values (avg, min, max, sum, etc.) with their bucket time ranges.
/// Returns an ordered map of attribute name to aggregated instance(s).
#[derive(Debug)]
pub struct TemporalAggregationRenderer<'a> {
    rows: &'a [PgRow],
    compiled: &'a CompiledTemporalQuery,
}

impl<'a> TemporalAggregationRenderer<'a> {
    /// Create a renderer for the given row set.
    pub const fn new(rows: &'a [PgRow], compiled: &'a CompiledTemporalQuery) -> TemporalAggregationRenderer<'a> {
        TemporalAggregationRenderer { rows, compiled }
    }
}

impl<'a> Renderer for TemporalAggregationRenderer<'a> {
    type Output = IndexMap<String, Value>;

    fn render(self) -> Result<IndexMap<String, Value>, AndromedaError> {
        let TemporalAggregationRenderer { rows, compiled } = self;

        type GroupKey = (String, Option<String>);
        let mut groups: IndexMap<GroupKey, Vec<&PgRow>> = IndexMap::with_capacity(rows.len().min(64));

        for row in rows {
            let attr_name: String = row.try_get("attribute_name")?;
            let dataset_id: Option<String> = row.try_get("dataset_id")?;
            groups.entry((attr_name, dataset_id)).or_default().push(row);
        }

        let mut attrs: IndexMap<String, Vec<Value>> = IndexMap::with_capacity(groups.len());

        for ((attr_name, dataset_id), bucket_rows) in &groups {
            let first_row = bucket_rows
                .first()
                .ok_or_else(|| AndromedaError::internal("Empty bucket rows in aggregation".to_string()))?;
            let attr_type_raw: String = first_row.try_get("attribute_type")?;
            let value_type_raw: String = first_row.try_get("value_type")?;
            let value_type =
                ValueType::from_str(&value_type_raw).map_err(|e| AndromedaError::internal(format!("Invalid value type '{}': {}", value_type_raw, e)))?;

            let mut instance = serde_json::Map::with_capacity(2 + compiled.representation.aggr_methods.len());
            instance.insert("type".to_string(), json!(attr_type_raw));

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
                        AggrMethod::TotalCount => row.try_get::<i64, _>("agg_total_count").map_or(Value::Null, |v| json!(v)),
                        AggrMethod::DistinctCount => row.try_get::<i64, _>("agg_distinct_count").map_or(Value::Null, |v| json!(v)),
                        AggrMethod::Min => extract_typed_agg(row, value_type, "agg_min", "agg_min_str", "agg_min_dt"),
                        AggrMethod::Max => extract_typed_agg(row, value_type, "agg_max", "agg_max_str", "agg_max_dt"),
                        AggrMethod::Avg => row.try_get::<f64, _>("agg_avg").map_or(Value::Null, |v| json!(v)),
                        AggrMethod::Sum => row.try_get::<f64, _>("agg_sum").map_or(Value::Null, |v| json!(v)),
                        AggrMethod::SumSq => row.try_get::<f64, _>("agg_sumsq").map_or(Value::Null, |v| json!(v)),
                        AggrMethod::StdDev => row.try_get::<f64, _>("agg_stddev").map_or(Value::Null, |v| json!(v)),
                    };

                    let bucket_start: Value = row
                        .try_get::<DateTime<Utc>, _>("bucket_start")
                        .map_or(Value::Null, |dt| json!(format_datetime(dt)));
                    let bucket_end: Value = row
                        .try_get::<DateTime<Utc>, _>("bucket_end")
                        .map_or(Value::Null, |dt| json!(format_datetime(dt)));

                    triples.push(json!([val, bucket_start, bucket_end]));
                }

                instance.insert(key.to_string(), Value::Array(triples));
            }

            attrs.entry(attr_name.clone()).or_default().push(Value::Object(instance));
        }

        let mut result: IndexMap<String, Value> = IndexMap::with_capacity(attrs.len());
        for (attr_name, instances) in attrs {
            if instances.len() == 1 {
                if let Some(single) = instances.into_iter().next() {
                    result.insert(attr_name, single);
                }
            } else {
                result.insert(attr_name, Value::Array(instances));
            }
        }

        Ok(result)
    }
}
