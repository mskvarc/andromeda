//! The aggregations behind entity type and attribute discovery (clauses 5.7.5-5.7.10).
//!
//! Each statement aggregates across the whole `entities` table, so all of them
//! are keyset-paginated on their aggregate key. Query-plan assumption: the
//! server materialises the full aggregation before applying the keyset filter
//! and `LIMIT`, so the pagination caps transport volume rather than server-side
//! work. It exists to bound the response, not to make the aggregate cheaper.

use crate::{
    entity::{
        mapper::{stored_attribute_name, stored_type_name},
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_entity::attribute_summary::AttributeSummary;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_postgresql_schema::current::{E, Entities};
use andromeda_postgresql_seaquery::{
    aggregate::{array_agg_distinct, array_agg_distinct_filter, count_all, count_distinct},
    cast::cast,
    jsonb::{jsonb_array_elements_text, jsonb_each, jsonb_object_keys, jsonb_typeof},
    pg_array::{any, text_array},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    unnest::unnest_of,
};
use sea_query::{CaseStatement, Cond, Expr, Func, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_binder::SqlxBinder;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};

/// The `unnest(types)` table source, aliased `t`; its single column is `t`.
#[derive(Iden, Clone, Copy)]
#[iden = "t"]
struct T;

/// The lateral derived table of an entity's attributes, aliased `attrs`.
#[derive(Iden, Clone, Copy)]
#[iden = "attrs"]
struct Attrs;

/// The `jsonb_each` source the attribute derived table reads from.
#[derive(Iden, Clone, Copy)]
#[iden = "je"]
struct Je;

/// The lateral expansion of one attribute's `@type` IRIs, aliased `at`.
#[derive(Iden, Clone, Copy)]
#[iden = "at"]
struct At;

/// The lateral expansion of an entity's attribute keys, aliased `ak`.
#[derive(Iden, Clone, Copy)]
#[iden = "ak"]
struct Ak;

/// The `key` column `jsonb_each` projects.
#[derive(Iden, Clone, Copy)]
#[iden = "key"]
struct Key;

/// The `value` column a set-returning source projects.
#[derive(Iden, Clone, Copy)]
#[iden = "value"]
struct ValueColumn;

/// The attribute key an aggregate groups on.
#[derive(Iden, Clone, Copy)]
#[iden = "attr_key"]
struct AttrKey;

/// The attribute value node an aggregate reads `@type` from.
#[derive(Iden, Clone, Copy)]
#[iden = "attr_value"]
struct AttrValue;

/// The aggregated type IRI a type summary reports.
#[derive(Iden, Clone, Copy)]
#[iden = "type_iri"]
struct TypeIri;

/// The aggregated attribute-key array a type summary reports.
#[derive(Iden, Clone, Copy)]
#[iden = "attr_keys"]
struct AttrKeys;

/// The entity-count column an attribute summary reports.
#[derive(Iden, Clone, Copy)]
#[iden = "entity_count"]
struct EntityCount;

/// The entity-type-array column an attribute summary reports.
#[derive(Iden, Clone, Copy)]
#[iden = "entity_types"]
struct EntityTypes;

/// The attribute-type-array column an attribute summary reports.
#[derive(Iden, Clone, Copy)]
#[iden = "attribute_types"]
struct AttributeTypes;

/// The attribute container of a payload: `payload -> 0` when that node is a JSON
/// object, and the payload root otherwise.
///
/// An entity's attributes live under the first element of its expanded payload
/// array, except for the fragment shapes whose root is already the attribute
/// object; the `CASE` reads whichever holds the members. `payload` is passed as
/// a fresh expression each time so the node can appear in the test, the `THEN`
/// and the `ELSE` without sharing an owned value.
fn attribute_container(payload: &dyn Fn() -> SimpleExpr) -> SimpleExpr {
    let node = || payload().json_get(Expr::val(0).into());
    CaseStatement::new()
        .case(Expr::expr(jsonb_typeof(node())).eq("object"), node())
        .finally(payload())
        .into()
}

/// The keyset guard `$after::text IS NULL OR key > $after`: an absent cursor
/// admits every group, a present one only the groups past it.
fn keyset_after(key: SimpleExpr, after: Option<&str>) -> Cond {
    let after = after.map(str::to_owned);
    Cond::any()
        .add(Expr::expr(cast(Expr::val(after.clone()).into(), PostgresType::Text)).is_null())
        .add(Expr::expr(key).gt(Expr::val(after)))
}

impl EntityRepository {
    /// Return all distinct entity type IRIs present in the store.
    pub async fn types(&self) -> Result<Vec<EntityTypeName>, ContextStoreError> {
        let statement = Query::select()
            .distinct()
            .expr(SimpleExpr::from(unnest_of(Expr::col(Entities::Types).into())))
            .from(Entities::Table)
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let raw: Vec<String> = sqlx::query_scalar_with(&sql, values).fetch_all(&self.pool).await?;
        raw.into_iter().map(stored_type_name).collect()
    }

    /// Retrieve entity payloads filtered by type with optional pagination.
    pub async fn get_entities_by_type(&self, entity_type: &EntityTypeName, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, ContextStoreError> {
        let mut statement = Query::select();
        statement
            .column(Entities::Payload)
            .from(Entities::Table)
            .and_where(Expr::val(entity_type.as_str()).eq(any(Expr::col(Entities::Types).into())))
            .order_by(Entities::Id, Order::Asc)
            .offset(offset.unwrap_or(0).max(0) as u64);
        if let Some(limit) = limit {
            statement.limit(limit.max(0) as u64);
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(&sql, values).fetch_all(&self.pool).await?)
    }

    /// Retrieve all entity payloads with optional pagination.
    pub async fn get_all_entities(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, ContextStoreError> {
        let mut statement = Query::select();
        statement
            .column(Entities::Payload)
            .from(Entities::Table)
            .order_by(Entities::Id, Order::Asc)
            .offset(offset.unwrap_or(0).max(0) as u64);
        if let Some(limit) = limit {
            statement.limit(limit.max(0) as u64);
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(&sql, values).fetch_all(&self.pool).await?)
    }

    /// Count entities that have the given type IRI.
    pub async fn count_by_type(&self, entity_type: &EntityTypeName) -> Result<i64, ContextStoreError> {
        let statement = Query::select()
            .expr(count_all())
            .from(Entities::Table)
            .and_where(Expr::val(entity_type.as_str()).eq(any(Expr::col(Entities::Types).into())))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(&sql, values).fetch_one(&self.pool).await?)
    }

    /// Per-type attribute summaries, keyset-paginated on the aggregated type IRI.
    pub async fn get_entity_type_summaries(
        &self,
        after: Option<&EntityTypeName>,
        limit: u32,
    ) -> Result<Vec<(EntityTypeName, Vec<AttributeName>)>, ContextStoreError> {
        let (sql, values) = entity_type_summaries_statement(after.map(EntityTypeName::as_str), u64::from(limit)).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await?;

        rows.into_iter()
            .map(|row| {
                let type_iri = stored_type_name(row.get("type_iri"))?;
                let attr_keys: Vec<String> = row.get("attr_keys");
                let attr_names = attr_keys.into_iter().map(stored_attribute_name).collect::<Result<_, _>>()?;
                Ok((type_iri, attr_names))
            })
            .collect()
    }

    /// Attribute metadata aggregated at the SQL level, keyset-paginated on the
    /// attribute IRI.
    pub async fn get_attribute_summary(&self, after: Option<&AttributeName>, limit: u32) -> Result<Vec<AttributeSummary>, ContextStoreError> {
        let (sql, values) = attribute_summary_statement(after.map(AttributeName::as_str), u64::from(limit)).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_attribute_summary).collect()
    }

    /// Attribute metadata for a single attribute IRI.
    pub async fn get_attribute_info(&self, attr_iri: &AttributeName) -> Result<Option<AttributeSummary>, ContextStoreError> {
        let (sql, values) = attribute_info_statement(attr_iri.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(&sql, values).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_attribute_summary).transpose()
    }
}

/// The attribute-summary aggregate up to and including `GROUP BY`, shared by the
/// paginated summary and the single-attribute lookup.
///
/// It differs between the two only by the `key_predicate` that selects which
/// attribute keys of an entity feed the aggregate, and by the trailing keyset
/// clauses the caller appends. `entities e` is cross-joined with a lateral
/// derived table of its attributes and with `unnest(e.types)`, then a lateral
/// `@type` expansion is left-joined so an attribute with no declared type still
/// contributes its entity to the counts.
fn attribute_summary_base(key_predicate: SimpleExpr) -> SelectStatement {
    let attributes = Query::select()
        .expr_as(Expr::col((Je, Key)), AttrKey)
        .expr_as(Expr::col((Je, ValueColumn)), AttrValue)
        .from_function(jsonb_each(attribute_container(&|| Expr::col((E, Entities::Payload)).into())), Je)
        .and_where(key_predicate)
        .to_owned();

    let attribute_type_iri = SimpleExpr::from(Expr::col((Attrs, AttrValue)))
        .json_get(Expr::val(0).into())
        .json_get(Expr::val("@type").into());
    let attribute_types = Query::select().expr_as(jsonb_array_elements_text(attribute_type_iri), ValueColumn).to_owned();

    Query::select()
        .expr_as(Expr::col((Attrs, AttrKey)), AttrKey)
        .expr_as(count_distinct(Expr::col((E, Entities::Id)).into()), EntityCount)
        .expr_as(array_agg_distinct(Expr::col(T).into()), EntityTypes)
        .expr_as(
            array_agg_distinct_filter(Expr::col((At, ValueColumn)).into(), Expr::col((At, ValueColumn)).is_not_null()),
            AttributeTypes,
        )
        .from_as(Entities::Table, E)
        .from_function(unnest_of(Expr::col((E, Entities::Types)).into()), T)
        .join_lateral(JoinType::InnerJoin, attributes, Attrs, SimpleExpr::from(Expr::val(true)))
        .join_lateral(JoinType::LeftJoin, attribute_types, At, SimpleExpr::from(Expr::val(true)))
        .group_by_col((Attrs, AttrKey))
        .to_owned()
}

/// The paginated attribute summary: every non-keyword attribute, keyset-paged on
/// the attribute IRI.
fn attribute_summary_statement(after: Option<&str>, limit: u64) -> SelectStatement {
    let mut statement = attribute_summary_base(Expr::col((Je, Key)).not_like("@%"));
    statement
        .cond_having(keyset_after(Expr::col((Attrs, AttrKey)).into(), after))
        .order_by((Attrs, AttrKey), Order::Asc)
        .limit(limit);
    statement
}

/// The single-attribute lookup: the aggregate restricted to one attribute key,
/// which groups to at most one row and needs no keyset clauses.
fn attribute_info_statement(attr_iri: &str) -> SelectStatement {
    attribute_summary_base(Expr::col((Je, Key)).eq(attr_iri))
}

/// Build the per-type attribute-key summary, keyset-paged on the type IRI.
///
/// Each entity contributes one row per type it carries and per top-level
/// non-keyword attribute key it holds; the aggregate collects the distinct
/// attribute keys of each type into an array, empty when a type has none.
fn entity_type_summaries_statement(after: Option<&str>, limit: u64) -> SelectStatement {
    let keys = Query::select()
        .expr_as(
            SimpleExpr::from(jsonb_object_keys(attribute_container(&|| Expr::col(Entities::Payload).into()))),
            AttrKey,
        )
        .to_owned();

    let mut statement = Query::select();
    statement
        .expr_as(Expr::col(T), TypeIri)
        .expr_as(
            Func::coalesce([
                array_agg_distinct_filter(Expr::col((Ak, AttrKey)).into(), Expr::col((Ak, AttrKey)).is_not_null()),
                text_array(&[]),
            ]),
            AttrKeys,
        )
        .from(Entities::Table)
        .from_function(unnest_of(Expr::col(Entities::Types).into()), T)
        .join_lateral(JoinType::InnerJoin, keys, Ak, SimpleExpr::from(Expr::val(true)))
        .and_where(Expr::col((Ak, AttrKey)).not_like("@%"))
        .group_by_col(T)
        .cond_having(keyset_after(Expr::col(T).into(), after))
        .order_by(T, Order::Asc)
        .limit(limit);
    statement
}

/// An attribute has no `@type` when no instance of it declares one, which the
/// aggregate reports as `NULL` rather than an empty array.
///
/// The `@type` IRIs the payload carried are mapped to the eight NGSI-LD
/// attribute family names (clause 5.2.28); any value that is not one of them is
/// dropped, which is what the discovery response asks for.
fn row_to_attribute_summary(row: &PgRow) -> Result<AttributeSummary, ContextStoreError> {
    let attribute_types: Option<Vec<String>> = row.get("attribute_types");
    let entity_types: Vec<String> = row.get("entity_types");
    Ok(AttributeSummary {
        attr_iri: stored_attribute_name(row.get("attr_key"))?,
        entity_count: row.get("entity_count"),
        entity_types: entity_types.into_iter().map(stored_type_name).collect::<Result<_, _>>()?,
        attribute_types: attribute_types
            .unwrap_or_default()
            .iter()
            .filter_map(|iri| AttributeType::from_iri_suffix(iri))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use crate::entity::discovery::{attribute_info_statement, attribute_summary_statement, entity_type_summaries_statement};
    use sea_query::PostgresQueryBuilder;
    use sea_query_binder::SqlxBinder;

    #[test]
    fn the_paginated_summary_and_the_single_lookup_share_one_aggregate() {
        let (summary, _) = attribute_summary_statement(None, 10).build_sqlx(PostgresQueryBuilder);
        let (info, _) = attribute_info_statement("https://example.org/speed").build_sqlx(PostgresQueryBuilder);
        for shared in [r#"COUNT(DISTINCT "e"."id")"#, "jsonb_each", "INNER JOIN LATERAL", "LEFT JOIN LATERAL"] {
            assert!(summary.contains(shared), "summary missing {shared}: {summary}");
            assert!(info.contains(shared), "info missing {shared}: {info}");
        }
        assert!(summary.contains("NOT LIKE"), "the summary skips keywords: {summary}");
        assert!(info.contains(r#""je"."key" ="#), "the lookup pins one key: {info}");
        assert!(!info.contains("LIMIT"), "the single-attribute lookup groups to one row: {info}");
    }

    #[test]
    fn the_summary_filters_missing_attribute_types_out_of_the_aggregate() {
        let (summary, _) = attribute_summary_statement(None, 10).build_sqlx(PostgresQueryBuilder);
        assert!(
            summary.contains(r#"array_agg(DISTINCT "at"."value") FILTER (WHERE "at"."value" IS NOT NULL)"#),
            "{summary}"
        );
    }

    #[test]
    fn the_summary_is_keyset_paged_on_the_attribute_iri() {
        let (summary, values) = attribute_summary_statement(Some("https://example.org/a"), 10).build_sqlx(PostgresQueryBuilder);
        assert!(summary.contains(r#"HAVING ($9::text) IS NULL OR "attrs"."attr_key" > $10"#), "{summary}");
        assert!(summary.contains(r#"ORDER BY "attrs"."attr_key" ASC"#), "{summary}");
        assert!(summary.contains("LIMIT $11"), "{summary}");
        // Every literal the statement carries binds rather than interpolating —
        // the keyword-skip pattern, the container index and key, the join
        // conditions, the cursor (twice) and the limit.
        assert_eq!(values.0.0.len(), 11, "{summary}");
    }

    #[test]
    fn the_type_summary_walks_the_keys_laterally_and_coalesces_to_an_empty_array() {
        let (sql, _) = entity_type_summaries_statement(None, 10).build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_object_keys"), "{sql}");
        assert!(sql.contains("JOIN LATERAL"), "{sql}");
        assert!(
            sql.contains(r#"COALESCE(array_agg(DISTINCT "ak"."attr_key") FILTER (WHERE "ak"."attr_key" IS NOT NULL)"#),
            "{sql}"
        );
        assert!(sql.contains(r#"GROUP BY "t""#), "{sql}");
    }
}
