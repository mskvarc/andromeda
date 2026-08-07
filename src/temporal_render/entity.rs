//! Temporal entity assembly: [`TemporalEntityRenderer`] assembles a temporal
//! entity representation from decoded history rows (NGSI-LD clause 6.3.10).

use crate::{
    service::temporal::store::TemporalHistoryRow,
    temporal_render::history_row::{TimeRange, neutral_row_to_expanded_instance},
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::{Value, json};

type TemporalAttributeInstances = Vec<(Option<DateTime<Utc>>, Value)>;
type TemporalAttrMap = IndexMap<String, TemporalAttributeInstances>;

/// Output of [`TemporalEntityRenderer`]: the assembled entity JSON together
/// with pagination metadata needed for Content-Range header construction.
#[derive(Debug)]
pub struct TemporalEntityBuildResult {
    /// The fully-assembled temporal entity JSON value (expanded form).
    pub entity: Value,
    /// Whether the result set was truncated by the system-applied lastN cap.
    pub truncated: bool,
    /// The observed time range across all attribute instances.
    pub time_range: Option<TimeRange>,
    /// The maximum partition total across all attributes (for Content-Range size).
    pub max_partition_total: Option<i64>,
}

/// Assembles a temporal entity representation from decoded history rows.
///
/// Groups attribute instances by attribute name, applies time range cutting
/// for unsynchronized attributes (NGSI-LD clause 6.3.10), and emits the
/// entity JSON along with truncation/pagination metadata.
#[derive(Debug)]
pub struct TemporalEntityRenderer<'a> {
    entity_id: &'a str,
    entity_types: &'a [EntityTypeName],
    rows: &'a [TemporalHistoryRow],
    compiled: &'a CompiledTemporalQuery,
}

impl<'a> TemporalEntityRenderer<'a> {
    /// Create a renderer for the given entity and row set.
    pub const fn new(
        entity_id: &'a str,
        entity_types: &'a [EntityTypeName],
        rows: &'a [TemporalHistoryRow],
        compiled: &'a CompiledTemporalQuery,
    ) -> TemporalEntityRenderer<'a> {
        TemporalEntityRenderer {
            entity_id,
            entity_types,
            rows,
            compiled,
        }
    }
}

impl<'a> TemporalEntityRenderer<'a> {
    /// Assemble the temporal entity and the pagination metadata around it.
    pub fn render(self) -> TemporalEntityBuildResult {
        let TemporalEntityRenderer {
            entity_id,
            entity_types,
            rows,
            compiled,
        } = self;

        let mut attrs: TemporalAttrMap = IndexMap::with_capacity(rows.len().min(64));
        let mut truncated = false;
        let mut min_time: Option<DateTime<Utc>> = None;
        let mut max_time: Option<DateTime<Utc>> = None;
        let mut max_partition_total: Option<i64> = None;

        for row in rows {
            let attr_name = row.attribute_name.as_str().to_string();

            if compiled.instances.last_n_origin.is_system_capped()
                && let Some(last_n) = compiled.instances.last_n
                && let Some(total) = row.partition_total
            {
                max_partition_total = Some(max_partition_total.map_or(total, |m: i64| m.max(total)));
                if total > last_n as i64 {
                    truncated = true;
                }
            }

            let obs = row.observed_at.or(Some(row.time));
            if let Some(t) = obs {
                min_time = Some(min_time.map_or(t, |m: DateTime<Utc>| m.min(t)));
                max_time = Some(max_time.map_or(t, |m: DateTime<Utc>| m.max(t)));
            }

            let instance = neutral_row_to_expanded_instance(row, compiled.representation.sys_attrs);
            attrs.entry(attr_name).or_default().push((obs, instance));
        }

        if truncated && attrs.len() > 1 {
            let attr_ranges: Vec<(&String, DateTime<Utc>, DateTime<Utc>)> = attrs
                .iter()
                .filter_map(|(name, instances)| {
                    let times: Vec<DateTime<Utc>> = instances.iter().filter_map(|(t, _)| *t).collect();
                    let min = times.iter().min().copied()?;
                    let max = times.iter().max().copied()?;
                    Some((name, min, max))
                })
                .collect();

            if attr_ranges.len() > 1 {
                let global_min = attr_ranges.iter().map(|(_, min, _)| *min).min();
                let global_max = attr_ranges.iter().map(|(_, _, max)| *max).max();
                if let (Some(g_min), Some(g_max)) = (global_min, global_max) {
                    let has_gap = attr_ranges.iter().any(|(_, a_min, _)| attr_ranges.iter().any(|(_, _, b_max)| *b_max < *a_min));

                    if has_gap {
                        let cut_max = if compiled.instances.last_n_origin.is_user_specified() { g_max } else { g_min };
                        let keep_range = attr_ranges.iter().find(|(_, min, max)| *min <= cut_max && cut_max <= *max);

                        if let Some((_, keep_min, keep_max)) = keep_range {
                            let (keep_min, keep_max) = (*keep_min, *keep_max);
                            for (_name, instances) in attrs.iter_mut() {
                                instances.retain(|(t, _)| t.is_some_and(|t| t >= keep_min && t <= keep_max));
                            }
                            min_time = Some(keep_min);
                            max_time = Some(keep_max);
                        }
                    }
                }
            }
        }

        let mut entity = serde_json::Map::with_capacity(attrs.len() + 2);
        entity.insert("@id".to_string(), json!(entity_id));
        entity.insert("@type".to_string(), json!(entity_types));

        for (attr_name, instances) in attrs {
            let values: Vec<Value> = instances.into_iter().map(|(_, v)| v).collect();
            entity.insert(attr_name, Value::Array(values));
        }

        let time_range = min_time.zip(max_time);
        TemporalEntityBuildResult {
            entity: Value::Object(entity),
            truncated,
            time_range,
            max_partition_total,
        }
    }
}
