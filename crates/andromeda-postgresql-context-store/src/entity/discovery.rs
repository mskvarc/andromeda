//! The aggregations behind entity type and attribute discovery (clauses 5.7.5-5.7.10).
//!
//! Each statement aggregates across every entity's search and document rows,
//! so all of them are keyset-paginated on their aggregate key. Query-plan assumption: the
//! server materialises the full aggregation before applying the keyset filter
//! and `LIMIT`, so the pagination caps transport volume rather than server-side
//! work. It exists to bound the response, not to make the aggregate cheaper.

use crate::{
    entity::{
        mapper::{stored_attribute_name, stored_type_name},
        read::identity_with_documents,
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_entity::attribute_summary::AttributeSummary;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_postgresql_query::entity_type_id::EntityTypeId;
use andromeda_postgresql_schema::{
    current::{Entities, EntityDocuments, EntitySearch},
    name_dictionary::EntityTypeNames,
};
use andromeda_postgresql_seaquery::{
    aggregate::{array_agg_distinct, array_agg_distinct_filter, count_all, count_distinct},
    cast::cast,
    jsonb::{jsonb_array_elements_text, jsonb_each, jsonb_object_keys, jsonb_typeof},
    operator::{array_contains, array_contains_key},
    pg_array::text_array,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    unnest::unnest_of,
};
use sea_query::{CaseStatement, Cond, Expr, ExprTrait, Func, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::slice::from_ref;

/// The lateral `unnest(type_ids)` source, aliased `t`; its single column is `t`.
#[derive(Iden, Clone, Copy)]
#[iden = "t"]
struct T;

/// The `entity_search` row an aggregate reads its type keys from, aliased `s`.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `entity_documents` row an aggregate reads its payload from, aliased `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The `entity_type_names` dictionary joined to an unnested type key, aliased `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
struct N;

/// The dictionary name column the type aggregates read: `n.name`.
fn type_name() -> SimpleExpr {
    Expr::col((N, EntityTypeNames::Name))
}

/// Join the type dictionary to the unnested `type_ids` source, so every
/// type key an entity carries is read back as the IRI it interns.
fn join_type_names(statement: &mut SelectStatement) -> &mut SelectStatement {
    statement.join_as(
        JoinType::InnerJoin,
        EntityTypeNames::Table,
        N,
        Expr::col((N, EntityTypeNames::Id)).equals((T, T)),
    )
}

/// Expand the search row's `type_ids` laterally, one row per type key, as
/// `INNER JOIN LATERAL (SELECT UNNEST(s.type_ids) AS t) AS t ON TRUE`.
///
/// Lateral rather than a second `FROM` item: the document row is joined to
/// the search row with an `ON` condition, and a comma-separated `FROM` item
/// cannot be referenced from a later join's `ON`.
fn unnest_type_ids(statement: &mut SelectStatement) -> &mut SelectStatement {
    statement.join_lateral(
        JoinType::InnerJoin,
        Query::select()
            .expr_as(SimpleExpr::from(unnest_of(Expr::col((S, EntitySearch::TypeIds)))), T)
            .take(),
        T,
        SimpleExpr::from(Expr::val(true)),
    )
}

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
    let node = || payload().json_get(Expr::val(0));
    CaseStatement::new()
        .case(Expr::expr(jsonb_typeof(node())).eq("object"), node())
        .finally(payload())
        .into()
}

/// `FROM entity_search s JOIN entity_documents d ON d.entity_key = s.entity_key`,
/// the source every payload-walking aggregate reads: the type keys from the
/// search row, the attribute members from the document row.
fn search_with_documents(statement: &mut SelectStatement) -> &mut SelectStatement {
    statement.from_as(EntitySearch::Table, S).join_as(
        JoinType::Join,
        EntityDocuments::Table,
        D,
        Expr::col((D, EntityDocuments::EntityKey)).equals((S, EntitySearch::EntityKey)),
    )
}

/// The keyset guard `$after::text IS NULL OR key > $after`: an absent cursor
/// admits every group, a present one only the groups past it.
fn keyset_after(key: SimpleExpr, after: Option<&str>) -> Cond {
    let after = after.map(str::to_owned);
    Cond::any()
        .add(Expr::expr(cast(Expr::val(after.clone()), PostgresType::Text)).is_null())
        .add(Expr::expr(key).gt(Expr::val(after)))
}

impl EntityRepository {
    /// Return all distinct entity type IRIs present in the store.
    ///
    /// Driven from the dictionary rather than by unnesting every entity's
    /// `type_ids`: a type is reported when at least one entity carries its
    /// key, which the `type_ids` GIN index answers per dictionary row.
    pub async fn types(&self) -> Result<Vec<EntityTypeName>, ContextStoreError> {
        let (sql, values) = types_in_use_statement().build_sqlx(PostgresQueryBuilder);
        let raw: Vec<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        raw.into_iter().map(stored_type_name).collect()
    }

    /// The dictionary key of `entity_type`, when the broker has ever
    /// interned it. A type never interned was never carried by any entity.
    async fn type_id(&self, entity_type: &EntityTypeName) -> Result<Option<EntityTypeId>, ContextStoreError> {
        Ok(self.entity_type_names.lookup(from_ref(entity_type)).await?.get(entity_type))
    }

    /// Retrieve entity payloads filtered by type with optional pagination.
    pub async fn get_entities_by_type(&self, entity_type: &EntityTypeName, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, ContextStoreError> {
        let Some(type_id) = self.type_id(entity_type).await? else {
            return Ok(Vec::new());
        };
        let mut statement = identity_with_documents();
        statement
            .column((EntityDocuments::Table, EntityDocuments::Payload))
            .join(
                JoinType::Join,
                EntitySearch::Table,
                Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
            )
            .and_where(array_contains_key(Expr::col((EntitySearch::Table, EntitySearch::TypeIds)), type_id.get()))
            .order_by((Entities::Table, Entities::Id), Order::Asc)
            .offset(<i64 as Ord>::max(offset.unwrap_or(0), 0) as u64);
        if let Some(limit) = limit {
            statement.limit(<i64 as Ord>::max(limit, 0) as u64);
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?)
    }

    /// Retrieve all entity payloads with optional pagination.
    pub async fn get_all_entities(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, ContextStoreError> {
        let mut statement = identity_with_documents();
        statement
            .column((EntityDocuments::Table, EntityDocuments::Payload))
            .order_by((Entities::Table, Entities::Id), Order::Asc)
            .offset(<i64 as Ord>::max(offset.unwrap_or(0), 0) as u64);
        if let Some(limit) = limit {
            statement.limit(<i64 as Ord>::max(limit, 0) as u64);
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?)
    }

    /// Count entities that have the given type IRI.
    pub async fn count_by_type(&self, entity_type: &EntityTypeName) -> Result<i64, ContextStoreError> {
        let Some(type_id) = self.type_id(entity_type).await? else {
            return Ok(0);
        };
        let statement = Query::select()
            .expr(count_all())
            .from(EntitySearch::Table)
            .and_where(array_contains_key(Expr::col(EntitySearch::TypeIds), type_id.get()))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(&self.pool).await?)
    }

    /// Per-type attribute summaries, keyset-paginated on the aggregated type IRI.
    pub async fn get_entity_type_summaries(
        &self,
        after: Option<&EntityTypeName>,
        limit: u32,
    ) -> Result<Vec<(EntityTypeName, Vec<AttributeName>)>, ContextStoreError> {
        let (sql, values) = entity_type_summaries_statement(after.map(EntityTypeName::as_str), u64::from(limit)).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

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
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_attribute_summary).collect()
    }

    /// Attribute metadata for a single attribute IRI.
    pub async fn get_attribute_info(&self, attr_iri: &AttributeName) -> Result<Option<AttributeSummary>, ContextStoreError> {
        let (sql, values) = attribute_info_statement(attr_iri.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_attribute_summary).transpose()
    }
}

/// Every dictionary type at least one entity carries, in name order:
/// `SELECT n.name FROM entity_type_names n WHERE EXISTS (SELECT 1 FROM entity_search WHERE type_ids @> ARRAY(SELECT n.id::integer)) ORDER BY n.name`.
fn types_in_use_statement() -> SelectStatement {
    let mut carried = Query::select();
    carried.expr(Expr::val(1)).from(EntitySearch::Table).and_where(array_contains(
        Expr::col((EntitySearch::Table, EntitySearch::TypeIds)),
        Expr::col((N, EntityTypeNames::Id)),
        PostgresType::Integer,
    ));
    let mut statement = Query::select();
    statement
        .expr(type_name())
        .from_as(EntityTypeNames::Table, N)
        .and_where(Expr::exists(carried.take()))
        .order_by((N, EntityTypeNames::Name), Order::Asc);
    statement
}

/// The attribute-summary aggregate up to and including `GROUP BY`, shared by the
/// paginated summary and the single-attribute lookup.
///
/// It differs between the two only by the `key_predicate` that selects which
/// attribute keys of an entity feed the aggregate, and by the trailing keyset
/// clauses the caller appends. `entity_search s` joined to `entity_documents d`
/// is cross-joined with a lateral derived table of the payload's attributes
/// and with `unnest(s.type_ids)` joined to the type dictionary, then a lateral
/// `@type` expansion is left-joined so an attribute with no declared type still
/// contributes its entity to the counts. Entities are counted by surrogate
/// key, so the identity row is never read.
fn attribute_summary_base(key_predicate: SimpleExpr) -> SelectStatement {
    let attributes = Query::select()
        .expr_as(Expr::col((Je, Key)), AttrKey)
        .expr_as(Expr::col((Je, ValueColumn)), AttrValue)
        .from_function(jsonb_each(attribute_container(&|| Expr::col((D, EntityDocuments::Payload)))), Je)
        .and_where(key_predicate)
        .to_owned();

    let attribute_type_iri = SimpleExpr::from(Expr::col((Attrs, AttrValue)))
        .json_get(Expr::val(0))
        .json_get(Expr::val("@type"));
    let attribute_types = Query::select().expr_as(jsonb_array_elements_text(attribute_type_iri), ValueColumn).to_owned();

    let mut statement = Query::select();
    statement
        .expr_as(Expr::col((Attrs, AttrKey)), AttrKey)
        .expr_as(count_distinct(Expr::col((S, EntitySearch::EntityKey))), EntityCount)
        .expr_as(array_agg_distinct(type_name()), EntityTypes)
        .expr_as(
            array_agg_distinct_filter(Expr::col((At, ValueColumn)), Expr::col((At, ValueColumn)).is_not_null()),
            AttributeTypes,
        );
    search_with_documents(&mut statement);
    unnest_type_ids(&mut statement);
    join_type_names(&mut statement)
        .join_lateral(JoinType::InnerJoin, attributes, Attrs, SimpleExpr::from(Expr::val(true)))
        .join_lateral(JoinType::LeftJoin, attribute_types, At, SimpleExpr::from(Expr::val(true)))
        .group_by_col((Attrs, AttrKey));
    statement
}

/// The paginated attribute summary: every non-keyword attribute, keyset-paged on
/// the attribute IRI.
fn attribute_summary_statement(after: Option<&str>, limit: u64) -> SelectStatement {
    let mut statement = attribute_summary_base(Expr::col((Je, Key)).not_like("@%"));
    statement
        .cond_having(keyset_after(Expr::col((Attrs, AttrKey)), after))
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
            SimpleExpr::from(jsonb_object_keys(attribute_container(&|| Expr::col((D, EntityDocuments::Payload))))),
            AttrKey,
        )
        .to_owned();

    let mut statement = Query::select();
    statement.expr_as(type_name(), TypeIri).expr_as(
        Func::coalesce([
            array_agg_distinct_filter(Expr::col((Ak, AttrKey)), Expr::col((Ak, AttrKey)).is_not_null()),
            text_array(&[]),
        ]),
        AttrKeys,
    );
    search_with_documents(&mut statement);
    unnest_type_ids(&mut statement);
    join_type_names(&mut statement)
        .join_lateral(JoinType::InnerJoin, keys, Ak, SimpleExpr::from(Expr::val(true)))
        .and_where(Expr::col((Ak, AttrKey)).not_like("@%"))
        .group_by_col((N, EntityTypeNames::Name))
        .cond_having(keyset_after(type_name(), after))
        .order_by((N, EntityTypeNames::Name), Order::Asc)
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
    use crate::entity::discovery::{attribute_info_statement, attribute_summary_statement, entity_type_summaries_statement, types_in_use_statement};
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;

    #[test]
    fn the_paginated_summary_and_the_single_lookup_share_one_aggregate() {
        let (summary, _) = attribute_summary_statement(None, 10).build_sqlx(PostgresQueryBuilder);
        let (info, _) = attribute_info_statement("https://example.org/speed").build_sqlx(PostgresQueryBuilder);
        for shared in [
            r#"COUNT(DISTINCT "s"."entity_key")"#,
            r#"FROM "entity_search" AS "s" JOIN "entity_documents" AS "d" ON "d"."entity_key" = "s"."entity_key""#,
            "jsonb_each",
            "INNER JOIN LATERAL",
            "LEFT JOIN LATERAL",
        ] {
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
        assert!(summary.contains(r#"HAVING ($10::text) IS NULL OR "attrs"."attr_key" > $11"#), "{summary}");
        assert!(summary.contains(r#"ORDER BY "attrs"."attr_key" ASC"#), "{summary}");
        assert!(summary.contains("LIMIT $12"), "{summary}");
        // Every literal the statement carries binds rather than interpolating —
        // the keyword-skip pattern, the container index and key, the join
        // conditions, the cursor (twice) and the limit.
        assert_eq!(values.0.0.len(), 12, "{summary}");
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
        assert!(sql.contains(r#"GROUP BY "n"."name""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN LATERAL (SELECT UNNEST("s"."type_ids") AS "t") AS "t" ON $"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "entity_type_names" AS "n" ON "n"."id" = "t"."t""#),
            "every unnested type key is read back through the dictionary: {sql}"
        );
    }

    /// The summary reports type IRIs, never keys, so the unnested keys are
    /// joined to the dictionary before they are aggregated.
    #[test]
    fn the_attribute_summary_aggregates_type_names_through_the_dictionary() {
        let (summary, _) = attribute_summary_statement(None, 10).build_sqlx(PostgresQueryBuilder);
        assert!(summary.contains(r#"array_agg(DISTINCT "n"."name") AS "entity_types""#), "{summary}");
        assert!(
            summary.contains(r#"INNER JOIN LATERAL (SELECT UNNEST("s"."type_ids") AS "t") AS "t" ON $"#),
            "{summary}"
        );
        assert!(!summary.contains(r#""entities""#), "the identity row is never read: {summary}");
    }

    /// The type list is driven from the dictionary and filtered by an
    /// index-served containment probe, so it never unnests the whole table.
    #[test]
    fn the_types_in_use_are_the_dictionary_rows_some_entity_carries() {
        let sql = types_in_use_statement().to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT "n"."name" FROM "entity_type_names" AS "n" WHERE EXISTS(SELECT 1 FROM "entity_search" WHERE "entity_search"."type_ids" @> ARRAY(SELECT "n"."id"::integer)) ORDER BY "n"."name" ASC"#
        );
    }
}
