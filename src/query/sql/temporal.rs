//! SQL builder for temporal queries (Stage 1: entity IDs, Stage 2: history per entity).

use crate::{
    core::error::AndromedaError,
    domain::temporal::{AggrMethod, TemporalFormat, TimeProp, TimeRel},
    query::{
        compiled::{CompiledTemporalQuery, TypeExpr},
        sql::{BindValue, EntityFilterConditionsInput, build_entity_filter_conditions},
    },
};
use std::borrow::Cow;

const fn time_column_expr_stage1(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "COALESCE(ah.observed_at, ah.time)",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "ah.time",
    }
}

/// Result of Stage 1: entity IDs that match the filters.
#[derive(Debug)]
pub struct TemporalStage1Result<'a> {
    pub sql: String,
    pub count_sql: Option<String>,
    pub bind_values: Vec<BindValue<'a>>,
}

/// Build Stage 1 query: find entity IDs matching entity-level filters.
pub fn build_stage1_entity_ids<'a>(
    compiled: &'a CompiledTemporalQuery,
    binds: &mut Vec<BindValue<'a>>,
    param_idx: &mut usize,
) -> Result<TemporalStage1Result<'a>, AndromedaError> {
    // Temporal queries use a flat type list; convert to TypeExpr::Or for SQL generation.
    let type_expr = compiled.types.as_ref().map(|types| {
        if types.len() == 1 {
            TypeExpr::Type(types[0].clone()) // clone: owned string for TypeExpr leaf
        } else {
            TypeExpr::Or(types.iter().map(|t| TypeExpr::Type(t.clone())).collect()) // clone: owned strings for TypeExpr leaves
        }
    });

    let mut conditions = build_entity_filter_conditions(EntityFilterConditionsInput {
        ids: compiled.ids.as_deref(),
        id_pattern: compiled.id_pattern.as_ref().map(|p| p.as_str()),
        type_expr: type_expr.as_ref(),
        q_expr: compiled.q_expr.as_ref(),
        geo: compiled.geo.as_ref(),
        attrs: &[],
        scope_q: compiled.scope_q.as_ref().map(|s| s.as_str()),
        table_prefix: "e",
        binds,
        param_idx,
    })?;

    // Attribute filter: an.name = ANY($N)
    if !compiled.attr_filter.is_empty() {
        bind_condition!(
            conditions,
            binds,
            param_idx,
            "an.name = ANY(${})",
            BindValue::StringVec(Cow::Borrowed(compiled.attr_filter.as_slice()))
        );
    }

    // datasetId filter: handle @none for default instance
    if !compiled.dataset_id_filter.is_empty() {
        let has_none = compiled.dataset_id_filter.iter().any(|s| s == "@none");
        let non_none: Vec<String> = compiled.dataset_id_filter.iter().filter(|s| *s != "@none").cloned().collect();

        if has_none && non_none.is_empty() {
            conditions.push("tea.dataset_id IS NULL".to_string());
        } else if has_none && !non_none.is_empty() {
            conditions.push(format!("(tea.dataset_id = ANY(${}) OR tea.dataset_id IS NULL)", *param_idx));
            binds.push(BindValue::StringVec(Cow::Owned(non_none)));
            *param_idx += 1;
        } else {
            conditions.push(format!("tea.dataset_id = ANY(${})", *param_idx));
            binds.push(BindValue::StringVec(Cow::Owned(non_none)));
            *param_idx += 1;
        }
    }

    // Temporal existence check: only entities with data in the time range (errata #7)
    let (join_ah, temporal_conditions) = if let (Some(timerel), Some(time_at)) = (&compiled.timerel, &compiled.time_at) {
        let time_col = time_column_expr_stage1(compiled.timeproperty);
        let mut tc = Vec::new();
        match timerel {
            TimeRel::Before => {
                tc.push(format!("{} < ${}::timestamptz", time_col, *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
            }
            TimeRel::After => {
                tc.push(format!("{} >= ${}::timestamptz", time_col, *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
            }
            TimeRel::Between => {
                if let Some(end_time) = &compiled.end_time_at {
                    tc.push(format!("{} >= ${}::timestamptz", time_col, *param_idx));
                    binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                    *param_idx += 1;
                    tc.push(format!("{} < ${}::timestamptz", time_col, *param_idx));
                    binds.push(BindValue::String(Cow::Owned(end_time.to_rfc3339())));
                    *param_idx += 1;
                }
            }
        }
        (" JOIN attribute_history ah ON ah.tea_id = tea.id", tc)
    } else {
        ("", Vec::new())
    };

    for c in temporal_conditions {
        conditions.push(c);
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let base = format!(
        "SELECT DISTINCT tea.entity_id FROM temporal_entity_attributes tea \
         JOIN attribute_names an ON an.id = tea.attribute_name_id \
         LEFT JOIN entities e ON e.id = tea.entity_id{}",
        join_ah
    );
    let limit_offset = format!(" LIMIT {} OFFSET {}", compiled.limit, compiled.offset);
    let sql = format!("{}{}{}", base, where_clause, limit_offset);

    let count_sql = if compiled.count {
        Some(format!(
            "SELECT COUNT(DISTINCT tea.entity_id) as count FROM temporal_entity_attributes tea \
             JOIN attribute_names an ON an.id = tea.attribute_name_id \
             LEFT JOIN entities e ON e.id = tea.entity_id{}{}",
            join_ah, where_clause
        ))
    } else {
        None
    };

    Ok(TemporalStage1Result {
        sql,
        count_sql,
        bind_values: binds.clone(),
    })
}

/// Result of Stage 2: history query for a single entity.
#[derive(Debug)]
pub struct TemporalStage2Result<'a> {
    pub sql: String,
    pub bind_values: Vec<BindValue<'a>>,
}

/// Time column expression based on timeproperty.
const fn time_column_expr(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "COALESCE(ah.observed_at, ah.time)",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "ah.time",
    }
}

/// Build Stage 2 query: temporal history for a single entity.
pub fn build_temporal_history_query<'a>(
    entity_id_param: usize,
    compiled: &'a CompiledTemporalQuery,
    binds: &mut Vec<BindValue<'a>>,
    param_idx: &mut usize,
) -> Result<TemporalStage2Result<'a>, AndromedaError> {
    let time_col = time_column_expr(compiled.timeproperty);

    let mut conditions = vec![format!("tea.entity_id = ${}", entity_id_param)];

    // When timeproperty=deletedAt, only return tombstone instances
    if compiled.timeproperty == TimeProp::Deleted {
        conditions.push("ah.value_string = 'urn:ngsi-ld:null'".to_string());
    }

    // Temporal filters
    if let (Some(timerel), Some(time_at)) = (&compiled.timerel, &compiled.time_at) {
        match timerel {
            TimeRel::Before => {
                conditions.push(format!("{} < ${}::timestamptz", time_col, *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
            }
            TimeRel::After => {
                conditions.push(format!("{} >= ${}::timestamptz", time_col, *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
            }
            TimeRel::Between => {
                let end_time = compiled
                    .end_time_at
                    .as_ref()
                    .ok_or_else(|| AndromedaError::query_validation("endTimeAt required for between"))?;
                conditions.push(format!("{} >= ${}::timestamptz", time_col, *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
                conditions.push(format!("{} < ${}::timestamptz", time_col, *param_idx));
                binds.push(BindValue::String(Cow::Owned(end_time.to_rfc3339())));
                *param_idx += 1;
            }
        }
    }

    // Attribute filter
    if !compiled.attr_filter.is_empty() {
        bind_condition!(
            conditions,
            binds,
            param_idx,
            "an.name = ANY(${})",
            BindValue::StringVec(Cow::Borrowed(compiled.attr_filter.as_slice()))
        );
    }

    // datasetId filter
    if !compiled.dataset_id_filter.is_empty() {
        let has_none = compiled.dataset_id_filter.iter().any(|s| s == "@none");
        let non_none: Vec<String> = compiled.dataset_id_filter.iter().filter(|s| *s != "@none").cloned().collect();

        if has_none && non_none.is_empty() {
            conditions.push("tea.dataset_id IS NULL".to_string());
        } else if has_none && !non_none.is_empty() {
            conditions.push(format!("(tea.dataset_id = ANY(${}) OR tea.dataset_id IS NULL)", *param_idx));
            binds.push(BindValue::StringVec(Cow::Owned(non_none)));
            *param_idx += 1;
        } else {
            conditions.push(format!("tea.dataset_id = ANY(${})", *param_idx));
            binds.push(BindValue::StringVec(Cow::Owned(non_none)));
            *param_idx += 1;
        }
    }

    let where_clause = conditions.join(" AND ");

    match compiled.format {
        TemporalFormat::AggregatedValues => build_aggregation_query(entity_id_param, compiled, &where_clause, binds, param_idx),
        _ => build_history_query(entity_id_param, compiled, &where_clause, binds, param_idx),
    }
}

fn build_history_query<'a>(
    _entity_id_param: usize,
    compiled: &'a CompiledTemporalQuery,
    where_clause: &str,
    binds: &mut Vec<BindValue<'a>>,
    param_idx: &mut usize,
) -> Result<TemporalStage2Result<'a>, AndromedaError> {
    let time_col = time_column_expr(compiled.timeproperty);
    // Unprefixed version for use in CTE outer query where table aliases aren't available
    let time_col_unprefixed = match compiled.timeproperty {
        TimeProp::Observed => "COALESCE(observed_at, time)",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "time",
    };
    let base_cols = "ah.time, ah.tea_id,
       ah.value_number, ah.value_string, ah.value_bool,
       ST_AsGeoJSON(ah.value_geo) as value_geo_json,
       ah.value_object, ah.value_datetime, ah.value_uri,
       ah.instance_id, ah.observed_at, ah.metadata,
       an.name AS attribute_name, tea.attribute_type, tea.dataset_id, tea.value_type";

    let sql = if let Some(last_n) = compiled.last_n {
        let rn_param = *param_idx;
        binds.push(BindValue::I64(last_n as i64));
        *param_idx += 1;

        format!(
            r#"WITH ranked AS (
    SELECT {},
           ROW_NUMBER() OVER (PARTITION BY ah.tea_id ORDER BY {time_col} DESC) as rn,
           COUNT(*) OVER (PARTITION BY ah.tea_id) as partition_total
    FROM attribute_history ah
    JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
    JOIN attribute_names an ON an.id = tea.attribute_name_id
    WHERE {}
)
SELECT time, tea_id, value_number, value_string, value_bool, value_geo_json,
       value_object, value_datetime, value_uri, instance_id, observed_at, metadata,
       attribute_name, attribute_type, dataset_id, value_type, partition_total
FROM ranked WHERE rn <= ${}
ORDER BY attribute_name, {time_col_unprefixed} ASC"#,
            base_cols, where_clause, rn_param
        )
    } else {
        format!(
            r#"SELECT {}
FROM attribute_history ah
JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
JOIN attribute_names an ON an.id = tea.attribute_name_id
WHERE {}
ORDER BY an.name, {time_col} ASC"#,
            base_cols, where_clause
        )
    };

    Ok(TemporalStage2Result {
        sql,
        bind_values: binds.clone(),
    })
}

fn build_aggregation_query<'a>(
    _entity_id_param: usize,
    compiled: &'a CompiledTemporalQuery,
    where_clause: &str,
    binds: &mut Vec<BindValue<'a>>,
    param_idx: &mut usize,
) -> Result<TemporalStage2Result<'a>, AndromedaError> {
    let period = compiled
        .aggr_period_duration
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "PT0S" && *s != "P0D");

    let (group_expr, order_expr) = if let Some(p) = period {
        let period_param = *param_idx;
        binds.push(BindValue::String(Cow::Owned(p.to_string())));
        *param_idx += 1;

        // Use time_at as bucket origin when provided; otherwise use the earliest
        // data point so buckets align to the actual data range (not epoch).
        let bucket_expr = if let Some(time_at) = &compiled.time_at {
            let origin_param = *param_idx;
            binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
            *param_idx += 1;
            format!("time_bucket(${}::interval, ah.time, ${}::timestamptz)", period_param, origin_param)
        } else {
            // Subquery origin: earliest ah.time in this entity's data set
            format!(
                "time_bucket(${}::interval, ah.time, (SELECT MIN(ah2.time) FROM attribute_history ah2 JOIN temporal_entity_attributes tea2 ON tea2.id = ah2.tea_id WHERE tea2.entity_id = tea.entity_id))",
                period_param
            )
        };

        (
            format!(
                "{} AS bucket_start,
    {} + ${}::interval AS bucket_end",
                bucket_expr, bucket_expr, period_param
            ),
            "an.name, bucket_start".to_string(),
        )
    } else {
        (
            "NULL::timestamptz AS bucket_start, NULL::timestamptz AS bucket_end".to_string(),
            "an.name".to_string(),
        )
    };

    // Unified numeric expression for type-aware aggregation (NGSI-LD 4.5.19.1).
    // Covers Number (direct), Boolean (true=1, false=0), and Array (element count).
    // Returns NULL for String, non-array Object, DateTime, Uri, and Geometry,
    // which correctly produces NULL aggregation results (N/A per spec).
    const NUMERIC_EXPR: &str = "COALESCE(\
        ah.value_number, \
        ah.value_bool::int::float8, \
        CASE WHEN jsonb_typeof(ah.value_object) = 'array' \
            THEN jsonb_array_length(ah.value_object)::float8 \
            ELSE NULL \
        END)";

    let mut agg_exprs = Vec::new();
    for m in &compiled.aggr_methods {
        match m {
            AggrMethod::Avg => {
                agg_exprs.push(format!("AVG({NUMERIC_EXPR}) AS agg_avg"));
            }
            AggrMethod::Min => {
                agg_exprs.push(format!("MIN({NUMERIC_EXPR}) AS agg_min"));
                agg_exprs.push("MIN(ah.value_string) AS agg_min_str".to_string());
                agg_exprs.push("MIN(ah.value_datetime) AS agg_min_dt".to_string());
            }
            AggrMethod::Max => {
                agg_exprs.push(format!("MAX({NUMERIC_EXPR}) AS agg_max"));
                agg_exprs.push("MAX(ah.value_string) AS agg_max_str".to_string());
                agg_exprs.push("MAX(ah.value_datetime) AS agg_max_dt".to_string());
            }
            AggrMethod::Sum => {
                agg_exprs.push(format!("SUM({NUMERIC_EXPR}) AS agg_sum"));
            }
            AggrMethod::SumSq => {
                agg_exprs.push(format!("SUM(({NUMERIC_EXPR}) * ({NUMERIC_EXPR})) AS agg_sumsq"));
            }
            AggrMethod::StdDev => {
                agg_exprs.push(format!("STDDEV({NUMERIC_EXPR}) AS agg_stddev"));
            }
            AggrMethod::TotalCount => {
                agg_exprs.push("COUNT(*) AS agg_total_count".to_string());
            }
            AggrMethod::DistinctCount => {
                agg_exprs.push(
                    "COUNT(DISTINCT COALESCE(\
                        ah.value_number::text, ah.value_string, ah.value_bool::text, \
                        ah.value_datetime::text, ah.value_uri, ah.value_object::text\
                    )) AS agg_distinct_count"
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
        "an.name, tea.attribute_type, tea.dataset_id, tea.value_type, bucket_start"
    } else {
        "an.name, tea.attribute_type, tea.dataset_id, tea.value_type"
    };

    let sql = format!(
        r#"SELECT
    an.name AS attribute_name,
    tea.attribute_type,
    tea.dataset_id,
    tea.value_type,
    {},
    {}
FROM attribute_history ah
JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
JOIN attribute_names an ON an.id = tea.attribute_name_id
WHERE {}
GROUP BY {}
ORDER BY {}"#,
        group_expr, agg_select, where_clause, group_by, order_expr
    );

    Ok(TemporalStage2Result {
        sql,
        bind_values: binds.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        query::compiled::{CompiledTemporalQuery, LastNOrigin, ScopeQuery},
        renderer::SysAttrsInclusion,
    };
    use chrono::{DateTime, Utc};

    fn minimal_compiled() -> CompiledTemporalQuery {
        CompiledTemporalQuery {
            ids: None,
            id_pattern: None,
            types: None,
            q_expr: None,
            geo: None,
            timerel: None,
            time_at: None,
            end_time_at: None,
            timeproperty: TimeProp::Observed,
            attr_filter: Vec::new(),
            dataset_id_filter: Vec::new(),
            last_n: None,
            last_n_origin: LastNOrigin::None,
            format: TemporalFormat::Normalized,
            aggr_methods: Vec::new(),
            aggr_period_duration: None,
            limit: 20,
            offset: 0,
            count: false,
            sys_attrs: SysAttrsInclusion::Strip,
            scope_q: None,
        }
    }

    #[test]
    fn build_temporal_history_no_filters() {
        let compiled = minimal_compiled();
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("tea.entity_id = $1"));
        assert!(result.sql.contains("attribute_history"));
        assert!(result.sql.contains("JOIN attribute_names an ON an.id = tea.attribute_name_id"));
        assert!(result.sql.contains("ORDER BY an.name, COALESCE(ah.observed_at, ah.time) ASC"));
    }

    #[test]
    fn build_temporal_history_before() {
        let mut compiled = minimal_compiled();
        compiled.timerel = Some(TimeRel::Before);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("COALESCE(ah.observed_at, ah.time) < $2::timestamptz"));
    }

    #[test]
    fn build_temporal_history_after() {
        let mut compiled = minimal_compiled();
        compiled.timerel = Some(TimeRel::After);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("COALESCE(ah.observed_at, ah.time) >= $2::timestamptz"));
    }

    #[test]
    fn build_temporal_history_between() {
        let mut compiled = minimal_compiled();
        compiled.timerel = Some(TimeRel::Between);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        compiled.end_time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T12:00:00Z").unwrap().with_timezone(&Utc));
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains(">="));
        assert!(result.sql.contains("<"));
    }

    #[test]
    fn build_temporal_history_with_attrs() {
        let mut compiled = minimal_compiled();
        compiled.attr_filter = vec!["https://uri.etsi.org/ngsi-ld/default-context/speed".to_string()];
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("an.name = ANY($2)"));
    }

    #[test]
    fn build_temporal_history_with_last_n() {
        let mut compiled = minimal_compiled();
        compiled.last_n = Some(5);
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("WITH ranked"));
        assert!(result.sql.contains("ROW_NUMBER()"));
        assert!(result.sql.contains("rn <= $2"));
    }

    #[test]
    fn build_aggregation_query_with_period() {
        let mut compiled = minimal_compiled();
        compiled.format = TemporalFormat::AggregatedValues;
        compiled.aggr_methods = vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max];
        compiled.aggr_period_duration = Some("PT1H".to_string());
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("time_bucket"));
        assert!(result.sql.contains("bucket_start"));
        assert!(result.sql.contains("agg_avg"));
        assert!(result.sql.contains("JOIN attribute_names an ON an.id = tea.attribute_name_id"));
    }

    #[test]
    fn build_aggregation_query_whole_range() {
        let mut compiled = minimal_compiled();
        compiled.format = TemporalFormat::AggregatedValues;
        compiled.aggr_methods = vec![AggrMethod::TotalCount];
        let mut binds = vec![BindValue::String(Cow::Borrowed("urn:ngsi-ld:Vehicle:A1"))];
        let mut param_idx = 2;
        let result = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("agg_total_count"));
        assert!(result.sql.contains("GROUP BY"));
    }

    #[test]
    fn build_entity_id_selection_with_type() {
        let mut compiled = minimal_compiled();
        compiled.types = Some(vec!["https://uri.etsi.org/ngsi-ld/default-context/Vehicle".to_string()]);
        let mut binds = Vec::new();
        let mut param_idx = 1;
        let result = build_stage1_entity_ids(&compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("SELECT DISTINCT tea.entity_id"));
        assert!(result.sql.contains("e.types"));
        assert!(result.sql.contains("JOIN attribute_names an ON an.id = tea.attribute_name_id"));
    }

    #[test]
    fn stage1_includes_attribute_names_join() {
        let compiled = minimal_compiled();
        let mut binds = Vec::new();
        let mut param_idx = 1;
        let result = build_stage1_entity_ids(&compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("JOIN attribute_names an ON an.id = tea.attribute_name_id"));
    }

    #[test]
    fn stage1_includes_scope_q_filter() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/Madrid/Gardens".to_string()));
        let mut binds = Vec::new();
        let mut param_idx = 1;
        let result = build_stage1_entity_ids(&compiled, &mut binds, &mut param_idx).unwrap();
        assert!(result.sql.contains("scope_paths"), "SQL should reference scope_paths column");
        assert!(result.sql.contains("lquery"), "SQL should contain lquery cast");
        // Bind value should contain ltree-formatted pattern (dots not slashes)
        let has_ltree_bind = binds.iter().any(|b| match b {
            BindValue::String(s) => s.contains("Madrid.Gardens"),
            _ => false,
        });
        assert!(has_ltree_bind, "Bind values should contain 'Madrid.Gardens' in ltree format");
    }

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        // Per NGSI-LD: comma=OR, semicolon=AND => (/A AND /B) OR /C
        compiled.scope_q = Some(ScopeQuery::new("/A;/B,/C".to_string()));
        let mut binds = Vec::new();
        let mut param_idx = 1;
        let result = build_stage1_entity_ids(&compiled, &mut binds, &mut param_idx).unwrap();
        // Should have AND/OR grouping in generated SQL
        assert!(result.sql.contains("AND"), "SQL should contain AND for semicolon-separated scopes");
        assert!(result.sql.contains("OR"), "SQL should contain OR for comma-separated scopes");
    }

    #[test]
    fn stage1_scope_q_with_wildcards() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/Madrid/#".to_string()));
        let mut binds = Vec::new();
        let mut param_idx = 1;
        let _result = build_stage1_entity_ids(&compiled, &mut binds, &mut param_idx).unwrap();
        // The # wildcard should be converted to lquery wildcard *{1,}
        let has_wildcard_bind = binds.iter().any(|b| match b {
            BindValue::String(s) => s.contains("Madrid.*{1,}"),
            _ => false,
        });
        assert!(has_wildcard_bind, "Bind values should contain 'Madrid.*{{1,}}' for # wildcard");
    }
}
