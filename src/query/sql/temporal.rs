//! SQL builder for temporal queries (Stage 1: entity IDs, Stage 2: history per entity).
//!
//! Uses sea-query for type-safe SQL generation with automatic parameter binding.

use crate::{
    core::error::AndromedaError,
    domain::temporal::{AggrMethod, TemporalFormat, TimeProp, TimeRel},
    query::{
        compiled::{CompiledTemporalQuery, GeoQuery, TypeExpr},
        q::ast::Expr,
        sql::{
            BindValue,
            BuiltQuery,
            postgres_ext::{
                any_eq,
                jsonb_key_exists,
                regex_match,
                st_contains,
                st_disjoint,
                st_dwithin,
                st_equals,
                st_intersects,
                st_overlaps,
                st_within,
                validated_geom_from_geojson,
            },
            schema::{Ah, An, AttributeHistory, AttributeNames, E, Entities, Tea, TemporalEntityAttributes},
            scope_to_lquery,
        },
    },
};
use sea_query::{Alias, Condition, Expr as SeaExpr, Func, JoinType, PostgresQueryBuilder, Query};
use std::borrow::Cow;

/// Properly quotes a table.column reference for PostgreSQL.
///
/// Input like `e.payload` becomes `"e"."payload"`.
/// Input without a dot like `payload` becomes `"payload"`.
fn quote_col_ref(col_ref: &str) -> String {
    if let Some((table, col)) = col_ref.split_once('.') {
        format!("\"{}\".\"{}\"", table, col)
    } else {
        format!("\"{}\"", col_ref)
    }
}

/// Time column expression based on timeproperty (stage 1 - with ah prefix).
const fn time_column_expr_stage1(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "COALESCE(\"ah\".\"observed_at\", \"ah\".\"time\")",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "\"ah\".\"time\"",
    }
}

/// Time column expression based on timeproperty (stage 2 - with ah prefix).
const fn time_column_expr(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "COALESCE(\"ah\".\"observed_at\", \"ah\".\"time\")",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "\"ah\".\"time\"",
    }
}

/// Time column expression without table prefix (for CTE outer query).
const fn time_column_expr_unprefixed(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "COALESCE(\"observed_at\", \"time\")",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "\"time\"",
    }
}

/// Build Stage 1 query: find entity IDs matching entity-level filters.
pub fn build_stage1_entity_ids(compiled: &CompiledTemporalQuery) -> Result<BuiltQuery<'static>, AndromedaError> {
    // Build entity-level conditions (applied to entities table via alias "e")
    let entity_conditions = build_entity_conditions_for_temporal(compiled)?;

    // Build temporal-specific conditions
    let temporal_conditions = build_temporal_stage1_conditions(compiled)?;

    // Combine all conditions
    let mut all_conditions = entity_conditions;
    if !temporal_conditions.is_empty() {
        all_conditions = all_conditions.add(temporal_conditions);
    }

    // Build the main SELECT
    let mut select = Query::select();
    select
        .distinct()
        .expr_as(SeaExpr::col((Tea, TemporalEntityAttributes::EntityId)), Alias::new("entity_id"))
        .from_as(TemporalEntityAttributes::Table, Tea)
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            SeaExpr::col((An, AttributeNames::Id)).equals((Tea, TemporalEntityAttributes::AttributeNameId)),
        )
        .join_as(
            JoinType::LeftJoin,
            Entities::Table,
            E,
            SeaExpr::col((E, Entities::Id)).equals((Tea, TemporalEntityAttributes::EntityId)),
        );

    // Conditional JOIN to attribute_history when timerel is present
    let needs_ah_join = compiled.timerel.is_some() && compiled.time_at.is_some();
    if needs_ah_join {
        select.join_as(
            JoinType::Join,
            AttributeHistory::Table,
            Ah,
            SeaExpr::col((Ah, AttributeHistory::TeaId)).equals((Tea, TemporalEntityAttributes::Id)),
        );
    }

    select
        .cond_where(all_conditions.clone())
        .limit(compiled.limit as u64)
        .offset(compiled.offset as u64);

    // Build main query SQL
    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values);

    // Build count query if requested
    let count_sql = if compiled.count {
        let mut count_select = Query::select();
        count_select
            .expr(Func::count(SeaExpr::cust("DISTINCT \"tea\".\"entity_id\"")))
            .from_as(TemporalEntityAttributes::Table, Tea)
            .join_as(
                JoinType::Join,
                AttributeNames::Table,
                An,
                SeaExpr::col((An, AttributeNames::Id)).equals((Tea, TemporalEntityAttributes::AttributeNameId)),
            )
            .join_as(
                JoinType::LeftJoin,
                Entities::Table,
                E,
                SeaExpr::col((E, Entities::Id)).equals((Tea, TemporalEntityAttributes::EntityId)),
            );

        if needs_ah_join {
            count_select.join_as(
                JoinType::Join,
                AttributeHistory::Table,
                Ah,
                SeaExpr::col((Ah, AttributeHistory::TeaId)).equals((Tea, TemporalEntityAttributes::Id)),
            );
        }

        count_select.cond_where(all_conditions);
        let (count_sql_str, _) = count_select.build(PostgresQueryBuilder);
        Some(count_sql_str)
    } else {
        None
    };

    Ok(BuiltQuery { sql, count_sql, bind_values })
}

/// Build entity-level conditions for temporal queries (using "e" alias for entities table).
fn build_entity_conditions_for_temporal(compiled: &CompiledTemporalQuery) -> Result<Condition, AndromedaError> {
    let mut condition = Condition::all();

    // ID filter: e.id = ANY($N)
    if let Some(ids) = &compiled.ids {
        if !ids.is_empty() {
            condition = condition.add(SeaExpr::col((E, Entities::Id)).is_in(ids.clone()));
        }
    }

    // ID pattern filter: e.id ~ $N
    if let Some(pattern) = &compiled.id_pattern {
        condition = condition.add(regex_match(SeaExpr::col((E, Entities::Id)).into(), SeaExpr::val(pattern.as_str()).into()));
    }

    // Type expression filter
    if let Some(types) = &compiled.types {
        let type_expr = if types.len() == 1 {
            TypeExpr::Type(types[0].clone())
        } else {
            TypeExpr::Or(types.iter().map(|t| TypeExpr::Type(t.clone())).collect())
        };
        let type_cond = type_expr_to_condition(&type_expr);
        condition = condition.add(type_cond);
    }

    // Q expression filter
    if let Some(q_expr) = &compiled.q_expr {
        let q_cond = q_expr_to_condition(q_expr, "e.payload")?;
        condition = condition.add(q_cond);
    }

    // Geo filter
    if let Some(geo) = &compiled.geo {
        let geo_cond = geo_to_condition(geo, "e.payload")?;
        condition = condition.add(geo_cond);
    }

    Ok(condition)
}

/// Build temporal-specific conditions for Stage 1 (attribute filters, datasetId, timerel, scopeQ).
fn build_temporal_stage1_conditions(compiled: &CompiledTemporalQuery) -> Result<Condition, AndromedaError> {
    let mut condition = Condition::all();

    // Attribute filter: an.name = ANY($N)
    if !compiled.attr_filter.is_empty() {
        condition = condition.add(SeaExpr::col((An, AttributeNames::Name)).is_in(compiled.attr_filter.clone()));
    }

    // datasetId filter: handle @none for default instance
    if !compiled.dataset_id_filter.is_empty() {
        let has_none = compiled.dataset_id_filter.iter().any(|s| s == "@none");
        let non_none: Vec<String> = compiled.dataset_id_filter.iter().filter(|s| *s != "@none").cloned().collect();

        if has_none && non_none.is_empty() {
            condition = condition.add(SeaExpr::col((Tea, TemporalEntityAttributes::DatasetId)).is_null());
        } else if has_none && !non_none.is_empty() {
            let or_cond = Condition::any()
                .add(SeaExpr::col((Tea, TemporalEntityAttributes::DatasetId)).is_in(non_none))
                .add(SeaExpr::col((Tea, TemporalEntityAttributes::DatasetId)).is_null());
            condition = condition.add(or_cond);
        } else {
            condition = condition.add(SeaExpr::col((Tea, TemporalEntityAttributes::DatasetId)).is_in(non_none));
        }
    }

    // Temporal filters (timerel + time_at)
    if let (Some(timerel), Some(time_at)) = (&compiled.timerel, &compiled.time_at) {
        let time_col = time_column_expr_stage1(compiled.timeproperty);
        match timerel {
            TimeRel::Before => {
                condition = condition.add(SeaExpr::cust_with_exprs(
                    "$1 < $2::timestamptz",
                    [SeaExpr::cust(time_col), SeaExpr::val(time_at.to_rfc3339()).into()],
                ));
            }
            TimeRel::After => {
                condition = condition.add(SeaExpr::cust_with_exprs(
                    "$1 >= $2::timestamptz",
                    [SeaExpr::cust(time_col), SeaExpr::val(time_at.to_rfc3339()).into()],
                ));
            }
            TimeRel::Between => {
                condition = condition.add(SeaExpr::cust_with_exprs(
                    "$1 >= $2::timestamptz",
                    [SeaExpr::cust(time_col), SeaExpr::val(time_at.to_rfc3339()).into()],
                ));
                if let Some(end_time) = &compiled.end_time_at {
                    condition = condition.add(SeaExpr::cust_with_exprs(
                        "$1 < $2::timestamptz",
                        [SeaExpr::cust(time_col), SeaExpr::val(end_time.to_rfc3339()).into()],
                    ));
                }
            }
        }
    }

    // Historical scope filtering via EXISTS subquery against scope_history
    if let Some(scope_q) = &compiled.scope_q {
        let scope_cond = scope_q_to_temporal_condition(
            scope_q.as_str(),
            "\"tea\".\"entity_id\"",
            compiled.timerel.as_ref(),
            compiled.time_at.as_ref(),
            compiled.end_time_at.as_ref(),
        )?;
        condition = condition.add(scope_cond);
    }

    Ok(condition)
}

/// Build Stage 2 query: temporal history for a single entity.
///
/// The `entity_id_param` is the bind parameter index for the entity ID (e.g., 1 means $1).
pub fn build_temporal_history_query(entity_id_param: usize, compiled: &CompiledTemporalQuery) -> Result<BuiltQuery<'static>, AndromedaError> {
    let time_col = time_column_expr(compiled.timeproperty);
    let mut conditions = Vec::new();

    // Entity ID filter
    conditions.push(format!("\"tea\".\"entity_id\" = ${}", entity_id_param));

    // When timeproperty=deletedAt, only return tombstone instances
    if compiled.timeproperty == TimeProp::Deleted {
        conditions.push("\"ah\".\"value_string\" = 'urn:ngsi-ld:null'".to_string());
    }

    // Temporal filters
    let mut bind_values: Vec<BindValue<'static>> = Vec::new();
    let mut next_param = entity_id_param + 1;

    if let (Some(timerel), Some(time_at)) = (&compiled.timerel, &compiled.time_at) {
        match timerel {
            TimeRel::Before => {
                conditions.push(format!("{} < ${}::timestamptz", time_col, next_param));
                bind_values.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                next_param += 1;
            }
            TimeRel::After => {
                conditions.push(format!("{} >= ${}::timestamptz", time_col, next_param));
                bind_values.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                next_param += 1;
            }
            TimeRel::Between => {
                let end_time = compiled
                    .end_time_at
                    .as_ref()
                    .ok_or_else(|| AndromedaError::query_validation("endTimeAt required for between"))?;
                conditions.push(format!("{} >= ${}::timestamptz", time_col, next_param));
                bind_values.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                next_param += 1;
                conditions.push(format!("{} < ${}::timestamptz", time_col, next_param));
                bind_values.push(BindValue::String(Cow::Owned(end_time.to_rfc3339())));
                next_param += 1;
            }
        }
    }

    // Attribute filter
    if !compiled.attr_filter.is_empty() {
        conditions.push(format!("\"an\".\"name\" = ANY(${})", next_param));
        bind_values.push(BindValue::StringVec(Cow::Owned(compiled.attr_filter.clone())));
        next_param += 1;
    }

    // datasetId filter
    if !compiled.dataset_id_filter.is_empty() {
        let has_none = compiled.dataset_id_filter.iter().any(|s| s == "@none");
        let non_none: Vec<String> = compiled.dataset_id_filter.iter().filter(|s| *s != "@none").cloned().collect();

        if has_none && non_none.is_empty() {
            conditions.push("\"tea\".\"dataset_id\" IS NULL".to_string());
        } else if has_none && !non_none.is_empty() {
            conditions.push(format!("(\"tea\".\"dataset_id\" = ANY(${}) OR \"tea\".\"dataset_id\" IS NULL)", next_param));
            bind_values.push(BindValue::StringVec(Cow::Owned(non_none)));
        } else {
            conditions.push(format!("\"tea\".\"dataset_id\" = ANY(${})", next_param));
            bind_values.push(BindValue::StringVec(Cow::Owned(non_none)));
        }
    }

    let where_clause = conditions.join(" AND ");

    match compiled.format {
        TemporalFormat::AggregatedValues => build_aggregation_query_sql(compiled, &where_clause, bind_values),
        _ => build_history_query_sql(compiled, &where_clause, bind_values),
    }
}

/// Build history query SQL (with optional lastN CTE).
fn build_history_query_sql(
    compiled: &CompiledTemporalQuery,
    where_clause: &str,
    mut bind_values: Vec<BindValue<'static>>,
) -> Result<BuiltQuery<'static>, AndromedaError> {
    let time_col = time_column_expr(compiled.timeproperty);
    let time_col_unprefixed = time_column_expr_unprefixed(compiled.timeproperty);

    let base_cols = r#""ah"."time", "ah"."tea_id",
       "ah"."value_number", "ah"."value_string", "ah"."value_bool",
       ST_AsGeoJSON("ah"."value_geo") as value_geo_json,
       "ah"."value_object", "ah"."value_datetime", "ah"."value_uri",
       "ah"."instance_id", "ah"."observed_at", "ah"."metadata",
       "an"."name" AS attribute_name, "tea"."attribute_type", "tea"."dataset_id", "tea"."value_type""#;

    let sql = if let Some(last_n) = compiled.last_n {
        // Find the next available parameter index
        let rn_param = bind_values.len() + 2; // +1 for entity_id, +1 for 1-based indexing
        bind_values.push(BindValue::I64(last_n as i64));

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
SELECT "time", "tea_id", "value_number", "value_string", "value_bool", "value_geo_json",
       "value_object", "value_datetime", "value_uri", "instance_id", "observed_at", "metadata",
       "attribute_name", "attribute_type", "dataset_id", "value_type", "partition_total"
FROM ranked WHERE "rn" <= ${}
ORDER BY "attribute_name", {} ASC"#,
            base_cols, time_col, where_clause, rn_param, time_col_unprefixed
        )
    } else {
        format!(
            r#"SELECT {}
FROM "attribute_history" AS "ah"
JOIN "temporal_entity_attributes" AS "tea" ON "tea"."id" = "ah"."tea_id"
JOIN "attribute_names" AS "an" ON "an"."id" = "tea"."attribute_name_id"
WHERE {}
ORDER BY "an"."name", {} ASC"#,
            base_cols, where_clause, time_col
        )
    };

    Ok(BuiltQuery {
        sql,
        count_sql: None,
        bind_values,
    })
}

/// Build aggregation query SQL with time_bucket.
fn build_aggregation_query_sql(
    compiled: &CompiledTemporalQuery,
    where_clause: &str,
    mut bind_values: Vec<BindValue<'static>>,
) -> Result<BuiltQuery<'static>, AndromedaError> {
    let period = compiled
        .aggr_period_duration
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "PT0S" && *s != "P0D");

    // Calculate next param index (bind_values starts after entity_id which is $1)
    let mut next_param = bind_values.len() + 2;

    let (group_expr, order_expr) = if let Some(p) = period {
        let period_param = next_param;
        bind_values.push(BindValue::String(Cow::Owned(p.to_string())));
        next_param += 1;

        // Use time_at as bucket origin when provided; otherwise use the earliest
        // data point so buckets align to the actual data range (not epoch).
        let bucket_expr = if let Some(time_at) = &compiled.time_at {
            let origin_param = next_param;
            bind_values.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
            format!("time_bucket(${}::interval, \"ah\".\"time\", ${}::timestamptz)", period_param, origin_param)
        } else {
            // Subquery origin: earliest ah.time in this entity's data set
            format!(
                "time_bucket(${}::interval, \"ah\".\"time\", (SELECT MIN(ah2.time) FROM attribute_history ah2 JOIN temporal_entity_attributes tea2 ON tea2.id = ah2.tea_id WHERE tea2.entity_id = \"tea\".\"entity_id\"))",
                period_param
            )
        };

        (
            format!(
                "{} AS bucket_start,\n    {} + ${}::interval AS bucket_end",
                bucket_expr, bucket_expr, period_param
            ),
            "\"an\".\"name\", bucket_start".to_string(),
        )
    } else {
        (
            "NULL::timestamptz AS bucket_start, NULL::timestamptz AS bucket_end".to_string(),
            "\"an\".\"name\"".to_string(),
        )
    };

    // Unified numeric expression for type-aware aggregation (NGSI-LD 4.5.19.1).
    const NUMERIC_EXPR: &str = r#"COALESCE(
        "ah"."value_number",
        "ah"."value_bool"::int::float8,
        CASE WHEN jsonb_typeof("ah"."value_object") = 'array'
            THEN jsonb_array_length("ah"."value_object")::float8
            ELSE NULL
        END)"#;

    let mut agg_exprs = Vec::new();
    for m in &compiled.aggr_methods {
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
        bind_values,
    })
}

// ============================================================================
// Helper functions adapted from entity.rs for temporal queries
// ============================================================================

/// Converts a TypeExpr to sea-query Condition (uses e.types column).
fn type_expr_to_condition(expr: &TypeExpr) -> Condition {
    match expr {
        TypeExpr::Type(iri) => {
            // $N = ANY(e.types)
            Condition::all().add(any_eq(SeaExpr::val(iri.as_str()).into(), SeaExpr::col((E, Entities::Types)).into()))
        }
        TypeExpr::And(children) => {
            let mut cond = Condition::all();
            for child in children {
                cond = cond.add(type_expr_to_condition(child));
            }
            cond
        }
        TypeExpr::Or(children) => {
            let mut cond = Condition::any();
            for child in children {
                cond = cond.add(type_expr_to_condition(child));
            }
            cond
        }
    }
}

/// Converts Q expression (NGSI-LD query language) to sea-query Condition.
fn q_expr_to_condition(expr: &Expr, payload_col: &str) -> Result<Condition, AndromedaError> {
    use crate::query::q::ast::{AttrPath, CmpOp, CompareValue, Predicate};

    fn pred_to_condition(pred: &Predicate, payload_col: &str) -> Result<Condition, AndromedaError> {
        let quoted_col = quote_col_ref(payload_col);
        match pred {
            Predicate::Exists(path) => {
                // payload->0 ? $N
                Ok(Condition::all().add(jsonb_key_exists(
                    SeaExpr::cust(format!("{}->0", quoted_col)),
                    SeaExpr::val(path.attr.as_str()).into(),
                )))
            }
            Predicate::Compare { path, op, rhs } => compare_to_condition(path, *op, rhs, payload_col),
            Predicate::MatchPattern { path, pattern } => {
                let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
                let sql = format!(
                    "jsonb_path_exists({}, '{}') AND (jsonb_path_query_first({}, '{}')#>>'{{}}')::text ~ $1",
                    quoted_col, jsonpath, quoted_col, jsonpath
                );
                Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [pattern.clone()])))
            }
            Predicate::NotMatchPattern { path, pattern } => {
                let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
                let sql = format!(
                    "NOT (jsonb_path_exists({}, '{}') AND (jsonb_path_query_first({}, '{}')#>>'{{}}')::text ~ $1)",
                    quoted_col, jsonpath, quoted_col, jsonpath
                );
                Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [pattern.clone()])))
            }
        }
    }

    fn compare_to_condition(path: &AttrPath, op: CmpOp, rhs: &CompareValue, payload_col: &str) -> Result<Condition, AndromedaError> {
        let quoted_col = quote_col_ref(payload_col);
        let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
        let extract_expr = format!("jsonb_path_query_first({}, '{}')", quoted_col, jsonpath);

        match rhs {
            CompareValue::Scalar(scalar) => {
                let op_str = cmp_op_sql(op);
                let (cast, bind_val) = scalar_to_sql_bind(scalar);
                let has_object_jsonpath =
                    value_extraction_jsonpath_with_predicate(&path.attr, &path.sub_attrs, &path.value_path, "https://uri.etsi.org/ngsi-ld/hasObject");
                let extract_object = format!("jsonb_path_query_first({}, '{}')", quoted_col, has_object_jsonpath);
                let sql = format!(
                    "(COALESCE(({} #>> '{{}}'), ({} #>> '{{}}')){}  {} $1{})",
                    extract_expr, extract_object, cast, op_str, cast
                );
                Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [bind_val])))
            }
            CompareValue::Range { min, max } => {
                let (cast, bind_min) = scalar_to_sql_bind(min);
                let (_, bind_max) = scalar_to_sql_bind(max);
                match op {
                    CmpOp::Equal => {
                        let sql = format!(
                            "(({} #>> '{{}}')){}  >= $1{} AND (({} #>> '{{}}')){}  <= $2{}",
                            extract_expr, cast, cast, extract_expr, cast, cast
                        );
                        Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [bind_min, bind_max])))
                    }
                    CmpOp::Unequal => {
                        let sql = format!(
                            "(({} #>> '{{}}')){}  < $1{} OR (({} #>> '{{}}')){}  > $2{}",
                            extract_expr, cast, cast, extract_expr, cast, cast
                        );
                        Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [bind_min, bind_max])))
                    }
                    _ => Err(AndromedaError::query_validation("Range values only supported with == and != operators")),
                }
            }
            CompareValue::ValueList(values) => {
                let mut parts = Vec::new();
                let mut bind_vals = Vec::new();
                for (i, v) in values.iter().enumerate() {
                    let (cast, bind_val) = scalar_to_sql_bind(v);
                    parts.push(format!("(({} #>> '{{}}')){}  = ${}{}", extract_expr, cast, i + 1, cast));
                    bind_vals.push(bind_val);
                }
                let joined = parts.join(" OR ");
                match op {
                    CmpOp::Equal => {
                        let sql = format!("({})", joined);
                        Ok(Condition::all().add(SeaExpr::cust_with_values(sql, bind_vals)))
                    }
                    CmpOp::Unequal => {
                        let sql = format!("NOT ({})", joined);
                        Ok(Condition::all().add(SeaExpr::cust_with_values(sql, bind_vals)))
                    }
                    _ => Err(AndromedaError::query_validation("Value lists only supported with == and != operators")),
                }
            }
        }
    }

    match expr {
        Expr::And(terms) => {
            let mut cond = Condition::all();
            for term in terms {
                cond = cond.add(q_expr_to_condition(term, payload_col)?);
            }
            Ok(cond)
        }
        Expr::Or(terms) => {
            let mut cond = Condition::any();
            for term in terms {
                cond = cond.add(q_expr_to_condition(term, payload_col)?);
            }
            Ok(cond)
        }
        Expr::Pred(pred) => pred_to_condition(pred, payload_col),
    }
}

/// Geo filter to condition using PostGIS helpers.
fn geo_to_condition(geo: &GeoQuery, payload_col: &str) -> Result<Condition, AndromedaError> {
    use crate::{jsonld::vocab, query::compiled::GeoRel};

    let is_default_location = geo.geoproperty_iri == "https://uri.etsi.org/ngsi-ld/location";
    let quoted_col = quote_col_ref(payload_col);

    // Build geometry extraction expression
    let geo_extract_sql = if is_default_location {
        if let Some(prefix) = payload_col.strip_suffix(".payload") {
            format!("\"{}\".\"location\"", prefix)
        } else {
            "\"location\"".to_string()
        }
    } else {
        format!(
            "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first({}, '$[0].\"{}\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\"')::text)), 4326))",
            quoted_col,
            escape_jsonpath_key(&geo.geoproperty_iri)
        )
    };

    let geojson = build_geojson(&geo.geometry, &geo.coordinates)?;
    let geo_extract_expr = SeaExpr::cust(&geo_extract_sql).into();
    let ref_geom_expr = validated_geom_from_geojson(SeaExpr::val(&geojson).into());

    // Build spatial condition using PostGIS helpers
    let spatial_expr = match &geo.georel {
        GeoRel::Within => st_within(geo_extract_expr, ref_geom_expr),
        GeoRel::Contains => st_contains(geo_extract_expr, ref_geom_expr),
        GeoRel::Intersects => st_intersects(geo_extract_expr, ref_geom_expr),
        GeoRel::Disjoint => st_disjoint(geo_extract_expr, ref_geom_expr),
        GeoRel::Equals => st_equals(geo_extract_expr, ref_geom_expr),
        GeoRel::Overlaps => st_overlaps(geo_extract_expr, ref_geom_expr),
        GeoRel::Near { max_distance, min_distance } => {
            let geo_col_sql = if is_default_location {
                if let Some(prefix) = payload_col.strip_suffix(".payload") {
                    format!("\"{}\".\"location_geo\"", prefix)
                } else {
                    "\"location_geo\"".to_string()
                }
            } else {
                format!("{}::geography", geo_extract_sql)
            };
            let ref_geom_geo_sql = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326))::geography", geojson);

            let mut parts = Vec::new();
            if let Some(max) = max_distance {
                parts.push(st_dwithin(SeaExpr::cust(&geo_col_sql).into(), SeaExpr::cust(&ref_geom_geo_sql).into(), *max));
            }
            if let Some(min) = min_distance {
                parts.push(SeaExpr::cust(format!("ST_Distance({}, {}) >= {}", geo_col_sql, ref_geom_geo_sql, min)).into());
            }
            if parts.is_empty() {
                return Err(AndromedaError::query_validation("near georel requires maxDistance or minDistance"));
            }
            if parts.len() == 1 {
                parts.remove(0)
            } else {
                let (sql1, _) = Query::select().expr(parts[0].clone()).build(PostgresQueryBuilder);
                let (sql2, _) = Query::select().expr(parts[1].clone()).build(PostgresQueryBuilder);
                SeaExpr::cust(format!("({} AND {})", sql1.trim_start_matches("SELECT "), sql2.trim_start_matches("SELECT "))).into()
            }
        }
    };

    // For default location, add expiration guard
    if is_default_location {
        let quoted_col = quote_col_ref(payload_col);
        let location_iri = vocab::expanded::LOCATION.as_str();
        let expires_at_iri = vocab::expanded::EXPIRES_AT.as_str();
        let guard_sql = format!(
            "EXISTS (\
                SELECT 1 FROM jsonb_array_elements({}->0->'{}') AS inst(elem) \
                WHERE (inst.elem #>> '{{{},0,@value}}') IS NULL \
                   OR (inst.elem #>> '{{{},0,@value}}')::timestamptz > NOW()\
            )",
            quoted_col, location_iri, expires_at_iri, expires_at_iri
        );
        Ok(Condition::all().add(spatial_expr).add(SeaExpr::cust(guard_sql)))
    } else {
        Ok(Condition::all().add(spatial_expr))
    }
}

/// Generate a temporal scope condition with EXISTS subquery against scope_history.
fn scope_q_to_temporal_condition(
    scope_q: &str,
    entity_id_col: &str,
    timerel: Option<&TimeRel>,
    time_at: Option<&chrono::DateTime<chrono::Utc>>,
    end_time_at: Option<&chrono::DateTime<chrono::Utc>>,
) -> Result<Condition, AndromedaError> {
    let or_terms: Vec<&str> = scope_q.split(',').map(str::trim).collect();
    let mut or_cond = Condition::any();

    for or_term in &or_terms {
        let and_terms: Vec<&str> = or_term.split(';').map(str::trim).collect();
        if and_terms.len() == 1 {
            let exists = single_scope_to_temporal_exists(and_terms[0], entity_id_col, timerel, time_at, end_time_at)?;
            or_cond = or_cond.add(exists);
        } else {
            let mut and_cond = Condition::all();
            for and_term in &and_terms {
                let exists = single_scope_to_temporal_exists(and_term, entity_id_col, timerel, time_at, end_time_at)?;
                and_cond = and_cond.add(exists);
            }
            or_cond = or_cond.add(and_cond);
        }
    }

    // If only one OR term, return it directly
    if or_terms.len() == 1 {
        let and_terms: Vec<&str> = or_terms[0].split(';').map(str::trim).collect();
        if and_terms.len() == 1 {
            return single_scope_to_temporal_exists(and_terms[0], entity_id_col, timerel, time_at, end_time_at);
        }
    }

    Ok(or_cond)
}

/// Generate EXISTS subquery for a single scope pattern against scope_history.
fn single_scope_to_temporal_exists(
    scope: &str,
    entity_id_col: &str,
    timerel: Option<&TimeRel>,
    time_at: Option<&chrono::DateTime<chrono::Utc>>,
    end_time_at: Option<&chrono::DateTime<chrono::Utc>>,
) -> Result<Condition, AndromedaError> {
    let lquery = scope_to_lquery(scope);

    let mut time_conditions = Vec::new();
    let mut bind_vals: Vec<String> = vec![lquery.clone()];

    if let (Some(timerel), Some(time_at)) = (timerel, time_at) {
        match timerel {
            TimeRel::Before => {
                time_conditions.push(format!("\"sh\".\"time\" < ${}::timestamptz", bind_vals.len() + 1));
                bind_vals.push(time_at.to_rfc3339());
            }
            TimeRel::After => {
                time_conditions.push(format!("\"sh\".\"time\" >= ${}::timestamptz", bind_vals.len() + 1));
                bind_vals.push(time_at.to_rfc3339());
            }
            TimeRel::Between => {
                time_conditions.push(format!("\"sh\".\"time\" >= ${}::timestamptz", bind_vals.len() + 1));
                bind_vals.push(time_at.to_rfc3339());
                if let Some(end_time) = end_time_at {
                    time_conditions.push(format!("\"sh\".\"time\" < ${}::timestamptz", bind_vals.len() + 1));
                    bind_vals.push(end_time.to_rfc3339());
                }
            }
        }
    }

    let time_clause = if time_conditions.is_empty() {
        String::new()
    } else {
        format!(" AND {}", time_conditions.join(" AND "))
    };

    let sql = format!(
        "EXISTS (SELECT 1 FROM \"scope_history\" AS \"sh\" WHERE \"sh\".\"entity_id\" = {} AND \"sh\".\"scope_path\" ~ $1::lquery{})",
        entity_id_col, time_clause
    );

    Ok(Condition::all().add(SeaExpr::cust_with_values(sql, bind_vals)))
}

// ============================================================================
// Utility functions
// ============================================================================

fn value_extraction_jsonpath(attr_iri: &str, sub_attrs: &[String], value_path: &[String]) -> String {
    value_extraction_jsonpath_with_predicate(attr_iri, sub_attrs, value_path, "https://uri.etsi.org/ngsi-ld/hasValue")
}

fn value_extraction_jsonpath_with_predicate(attr_iri: &str, sub_attrs: &[String], value_path: &[String], predicate_iri: &str) -> String {
    let mut path = format!("$[0].\"{}\"", escape_jsonpath_key(attr_iri));

    for sub in sub_attrs {
        path.push_str(&format!("[0].\"{}\"", escape_jsonpath_key(sub)));
    }

    path.push_str(&format!("[0].\"{}\"[0]", escape_jsonpath_key(predicate_iri)));

    let value_key = if predicate_iri == "https://uri.etsi.org/ngsi-ld/hasObject" {
        "@id"
    } else {
        "@value"
    };

    path.push_str(&format!(".\"{}\"", value_key));
    for key in value_path {
        path.push_str(&format!(".\"{}\"", escape_jsonpath_key(key)));
    }

    path
}

fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

const fn cmp_op_sql(op: crate::query::q::ast::CmpOp) -> &'static str {
    use crate::query::q::ast::CmpOp;
    match op {
        CmpOp::Equal => "=",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

fn scalar_to_sql_bind(scalar: &crate::query::q::ast::Scalar) -> (&'static str, String) {
    use crate::query::q::ast::Scalar;
    match scalar {
        Scalar::Number(n) => ("::numeric", n.to_string()),
        Scalar::Bool(b) => ("::boolean", b.to_string()),
        Scalar::Str(s) => ("", s.clone()),
        Scalar::DateTime(s) | Scalar::Date(s) | Scalar::Time(s) => ("", s.clone()),
    }
}

fn build_geojson(geometry: &str, coordinates: &serde_json::Value) -> Result<String, AndromedaError> {
    let geojson = serde_json::json!({
        "type": geometry,
        "coordinates": coordinates
    });
    Ok(geojson.to_string())
}

/// Convert sea-query Values to BindValue vector for repository compatibility.
fn sea_values_to_bind_values(values: sea_query::Values) -> Vec<BindValue<'static>> {
    values
        .0
        .into_iter()
        .map(|v| match v {
            sea_query::Value::String(Some(s)) => BindValue::String(Cow::Owned(s.to_string())),
            sea_query::Value::Int(Some(i)) => BindValue::I64(i64::from(i)),
            sea_query::Value::BigInt(Some(i)) => BindValue::I64(i),
            sea_query::Value::BigUnsigned(Some(u)) => BindValue::I64(u as i64),
            sea_query::Value::Unsigned(Some(u)) => BindValue::I64(i64::from(u)),
            sea_query::Value::TinyInt(Some(i)) => BindValue::I64(i64::from(i)),
            sea_query::Value::TinyUnsigned(Some(u)) => BindValue::I64(i64::from(u)),
            sea_query::Value::SmallInt(Some(i)) => BindValue::I64(i64::from(i)),
            sea_query::Value::SmallUnsigned(Some(u)) => BindValue::I64(i64::from(u)),
            sea_query::Value::Bool(Some(b)) => BindValue::String(Cow::Owned(b.to_string())),
            sea_query::Value::Double(Some(f)) => BindValue::String(Cow::Owned(f.to_string())),
            sea_query::Value::Float(Some(f)) => BindValue::String(Cow::Owned(f.to_string())),
            other => panic!("unexpected sea_query::Value variant in bind conversion: {:?}", other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
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
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains("\"tea\".\"entity_id\" = $1"));
        assert!(result.sql.contains("attribute_history"));
        assert!(result.sql.contains("JOIN \"attribute_names\""));
        assert!(result.sql.contains("ORDER BY"));
    }

    #[test]
    fn build_temporal_history_before() {
        let mut compiled = minimal_compiled();
        compiled.timerel = Some(TimeRel::Before);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains("< $2::timestamptz"));
    }

    #[test]
    fn build_temporal_history_after() {
        let mut compiled = minimal_compiled();
        compiled.timerel = Some(TimeRel::After);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains(">= $2::timestamptz"));
    }

    #[test]
    fn build_temporal_history_between() {
        let mut compiled = minimal_compiled();
        compiled.timerel = Some(TimeRel::Between);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z").unwrap().with_timezone(&Utc));
        compiled.end_time_at = Some(DateTime::parse_from_rfc3339("2024-01-15T12:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains(">="));
        assert!(result.sql.contains("<"));
    }

    #[test]
    fn build_temporal_history_with_attrs() {
        let mut compiled = minimal_compiled();
        compiled.attr_filter = vec!["https://uri.etsi.org/ngsi-ld/default-context/speed".to_string()];
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains("\"an\".\"name\" = ANY($2)"));
    }

    #[test]
    fn build_temporal_history_with_last_n() {
        let mut compiled = minimal_compiled();
        compiled.last_n = Some(5);
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains("WITH ranked"));
        assert!(result.sql.contains("ROW_NUMBER()"));
        assert!(result.sql.contains("\"rn\" <="));
    }

    #[test]
    fn build_aggregation_query_with_period() {
        let mut compiled = minimal_compiled();
        compiled.format = TemporalFormat::AggregatedValues;
        compiled.aggr_methods = vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max];
        compiled.aggr_period_duration = Some("PT1H".to_string());
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains("time_bucket"));
        assert!(result.sql.contains("bucket_start"));
        assert!(result.sql.contains("agg_avg"));
        assert!(result.sql.contains("JOIN \"attribute_names\""));
    }

    #[test]
    fn build_aggregation_query_whole_range() {
        let mut compiled = minimal_compiled();
        compiled.format = TemporalFormat::AggregatedValues;
        compiled.aggr_methods = vec![AggrMethod::TotalCount];
        let result = build_temporal_history_query(1, &compiled).unwrap();
        assert!(result.sql.contains("agg_total_count"));
        assert!(result.sql.contains("GROUP BY"));
    }

    #[test]
    fn build_entity_id_selection_with_type() {
        let mut compiled = minimal_compiled();
        compiled.types = Some(vec!["https://uri.etsi.org/ngsi-ld/default-context/Vehicle".to_string()]);
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains("SELECT DISTINCT"));
        assert!(result.sql.contains("\"e\".\"types\""));
        assert!(result.sql.contains("JOIN \"attribute_names\""));
    }

    #[test]
    fn stage1_timerel_placeholder_is_offset_after_id_filters() {
        let mut compiled = minimal_compiled();
        compiled.ids = Some(vec!["urn:ngsi-ld:Vehicle:1".to_string(), "urn:ngsi-ld:Vehicle:2".to_string()]);
        compiled.timerel = Some(TimeRel::Before);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z").unwrap().with_timezone(&Utc));

        let result = build_stage1_entity_ids(&compiled).unwrap();

        assert!(
            !result.sql.contains("< $1::timestamptz"),
            "time placeholder must not collide with earlier id-filter placeholders"
        );
        assert!(
            result.sql.contains("::timestamptz"),
            "temporal comparison should cast bind value to timestamptz"
        );
    }

    #[test]
    fn stage1_includes_attribute_names_join() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains("JOIN \"attribute_names\""));
    }

    #[test]
    fn stage1_includes_scope_q_filter() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/Madrid/Gardens".to_string()));
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains("scope_history"), "SQL should reference scope_history table");
        assert!(result.sql.contains("lquery"), "SQL should contain lquery cast");
    }

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/A;/B,/C".to_string()));
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains("AND"), "SQL should contain AND for semicolon-separated scopes");
        assert!(result.sql.contains("OR"), "SQL should contain OR for comma-separated scopes");
    }

    #[test]
    fn stage1_scope_q_with_wildcards() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/Madrid/#".to_string()));
        let _result = build_stage1_entity_ids(&compiled).unwrap();
        // The wildcard is embedded in the bind values which sea-query manages
    }

    #[test]
    fn temporal_scope_q_generates_scope_history_exists() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/Madrid".to_string()));
        compiled.timerel = Some(TimeRel::Before);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-06-01T00:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains("scope_history"), "SQL should reference scope_history table");
    }

    #[test]
    fn temporal_scope_q_between() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/A".to_string()));
        compiled.timerel = Some(TimeRel::Between);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc));
        compiled.end_time_at = Some(DateTime::parse_from_rfc3339("2024-06-01T00:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains(">="), "Between should have >= on sh.time");
        assert!(result.sql.contains("<"), "Between should have < on sh.time");
    }

    #[test]
    fn temporal_scope_q_after() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/A/#".to_string()));
        compiled.timerel = Some(TimeRel::After);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-03-01T00:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains(">="), "After should have >= on sh.time");
    }

    #[test]
    fn temporal_scope_q_no_timerel() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/A".to_string()));
        // No timerel/time_at set
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(result.sql.contains("scope_history"), "Should still query scope_history");
    }

    #[test]
    fn temporal_scope_q_compound() {
        let mut compiled = minimal_compiled();
        compiled.scope_q = Some(ScopeQuery::new("/A;/B,/C".to_string()));
        compiled.timerel = Some(TimeRel::Before);
        compiled.time_at = Some(DateTime::parse_from_rfc3339("2024-06-01T00:00:00Z").unwrap().with_timezone(&Utc));
        let result = build_stage1_entity_ids(&compiled).unwrap();
        // /A;/B -> AND, ,/C -> OR
        assert!(result.sql.contains("AND"), "Semicolon scopes produce AND");
        assert!(result.sql.contains("OR"), "Comma scopes produce OR");
        // Each scope term should have its own EXISTS subquery
        let exists_count = result.sql.matches("EXISTS").count();
        pretty_assertions::assert_eq!(exists_count, 3, "Three scope terms should produce three EXISTS subqueries");
    }

    #[test]
    fn no_scope_q_unchanged() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled).unwrap();
        assert!(!result.sql.contains("scope_history"), "No scope_history reference without scopeQ");
    }
}
