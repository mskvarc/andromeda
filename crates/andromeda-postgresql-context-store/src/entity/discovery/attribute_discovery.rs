//! The attribute half of discovery (clauses 5.7.8 to 5.7.10).
//!
//! One aggregate answers both the paginated Attribute List and the
//! single-attribute lookup: they differ only in which attribute keys of an
//! entity feed it and in the keyset clauses the caller appends.

use crate::{
    entity::{
        discovery::{
            aggregate_source::{AttrKey, D, S, attribute_container, keyset_after, search_with_documents},
            type_dictionary::{join_type_names, type_name, unnest_type_ids},
        },
        mapper::{stored_attribute_name, stored_type_name},
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_context_space::{context_space_key::ContextSpaceKey, context_target::ContextTarget};
use andromeda_entity::attribute_summary::AttributeSummary;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_postgresql_schema::current::{EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::{
    aggregate::{array_agg_distinct, array_agg_distinct_filter, count_distinct},
    cast::cast,
    jsonb::{jsonb_array_elements_text, jsonb_build_array, jsonb_each, jsonb_typeof},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, Expr, ExprTrait, FunctionCall, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, postgres::PgRow};

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

/// The `key` column `jsonb_each` projects.
#[derive(Iden, Clone, Copy)]
#[iden = "key"]
struct Key;

/// The `value` column a set-returning source projects.
#[derive(Iden, Clone, Copy)]
#[iden = "value"]
struct ValueColumn;

/// The attribute value node an aggregate reads `@type` from.
#[derive(Iden, Clone, Copy)]
#[iden = "attr_value"]
struct AttrValue;

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

/// The `@type` IRIs one attribute node declares, as one text row each.
///
/// JSON-LD 1.1 section 4.1 admits both spellings of `@type` on a node object --
/// "a string or an array of strings" -- and only expansion normalises it to the
/// array form, so a stored node that carries the string spelling is well-formed
/// JSON-LD rather than a corrupt row. Reading it as an array unconditionally
/// raises PostgreSQL's `cannot extract elements from a scalar`, and because this
/// set is cross-joined into the aggregate, that failure takes the whole
/// Attribute List down rather than the one attribute that provoked it -- while
/// clause 5.7.9 answers an Attribute List and clause 5.5.6 admits no
/// `InternalError` for a well-formed read of data the broker itself stored. The
/// `CASE` therefore lifts the string spelling into the one-element array it
/// stands for, and reads every other JSON type -- an absent `@type` included --
/// as naming no type at all.
///
/// `attribute_type` is passed as a fresh expression each time because the node
/// appears in both tests and in two of the branches.
fn attribute_type_iris(attribute_type: &dyn Fn() -> SimpleExpr) -> FunctionCall {
    let spelling = CaseStatement::new()
        .case(Expr::expr(jsonb_typeof(attribute_type())).eq("array"), attribute_type())
        .case(Expr::expr(jsonb_typeof(attribute_type())).eq("string"), jsonb_build_array(attribute_type()))
        .finally(cast(text_literal("[]"), PostgresType::Jsonb));
    jsonb_array_elements_text(spelling.into())
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
fn attribute_summary_base(context_space_key: ContextSpaceKey, key_predicate: SimpleExpr) -> SelectStatement {
    let attributes = Query::select()
        .expr_as(Expr::col((Je, Key)), AttrKey)
        .expr_as(Expr::col((Je, ValueColumn)), AttrValue)
        .from_function(jsonb_each(attribute_container(&|| Expr::col((D, EntityDocuments::Payload)))), Je)
        .and_where(key_predicate)
        .to_owned();

    let attribute_type_iri = || {
        SimpleExpr::from(Expr::col((Attrs, AttrValue)))
            .json_get(Expr::val(0))
            .json_get(Expr::val("@type"))
    };
    let attribute_types = Query::select().expr_as(attribute_type_iris(&attribute_type_iri), ValueColumn).to_owned();

    let mut statement = Query::select();
    statement
        .expr_as(Expr::col((Attrs, AttrKey)), AttrKey)
        .expr_as(count_distinct(Expr::col((S, EntitySearch::EntityKey))), EntityCount)
        .expr_as(array_agg_distinct(type_name()), EntityTypes)
        .expr_as(
            array_agg_distinct_filter(Expr::col((At, ValueColumn)), Expr::col((At, ValueColumn)).is_not_null()),
            AttributeTypes,
        );
    search_with_documents(&mut statement, context_space_key);
    unnest_type_ids(&mut statement);
    join_type_names(&mut statement)
        .join_lateral(JoinType::InnerJoin, attributes, Attrs, SimpleExpr::from(Expr::val(true)))
        .join_lateral(JoinType::LeftJoin, attribute_types, At, SimpleExpr::from(Expr::val(true)))
        .group_by_col((Attrs, AttrKey));
    statement
}

/// The paginated attribute summary: every non-keyword attribute, keyset-paged on
/// the attribute IRI.
fn attribute_summary_statement(context_space_key: ContextSpaceKey, after: Option<&str>, limit: u64) -> SelectStatement {
    let mut statement = attribute_summary_base(context_space_key, Expr::col((Je, Key)).not_like("@%"));
    statement
        .cond_having(keyset_after(Expr::col((Attrs, AttrKey)), after))
        .order_by((Attrs, AttrKey), Order::Asc)
        .limit(limit);
    statement
}

/// The single-attribute lookup: the aggregate restricted to one attribute key,
/// which groups to at most one row and needs no keyset clauses.
fn attribute_info_statement(context_space_key: ContextSpaceKey, attr_iri: &str) -> SelectStatement {
    attribute_summary_base(context_space_key, Expr::col((Je, Key)).eq(attr_iri))
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
        entity_types: entity_types.iter().map(String::as_str).map(stored_type_name).collect::<Result<_, _>>()?,
        attribute_types: attribute_types
            .unwrap_or_default()
            .iter()
            .filter_map(|iri| AttributeType::from_iri_suffix(iri))
            .collect(),
    })
}

impl EntityRepository {
    /// Attribute metadata aggregated at the SQL level, keyset-paginated on the
    /// attribute IRI.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the aggregation fails, or when an
    /// aggregated row does not map to an [`AttributeSummary`] because its
    /// attribute IRI, type names or attribute types no longer parse.
    pub async fn get_attribute_summary(
        &self,
        target: &ContextTarget,
        after: Option<&AttributeName>,
        limit: u32,
    ) -> Result<Vec<AttributeSummary>, ContextStoreError> {
        let (sql, values) = attribute_summary_statement(target.key(), after.map(AttributeName::as_str), u64::from(limit)).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_attribute_summary).collect()
    }

    /// Attribute metadata for a single attribute IRI.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the aggregation fails, or when the
    /// aggregated row does not map to an [`AttributeSummary`] because its
    /// attribute IRI, type names or attribute types no longer parse.
    pub async fn get_attribute_info(&self, target: &ContextTarget, attr_iri: &AttributeName) -> Result<Option<AttributeSummary>, ContextStoreError> {
        let (sql, values) = attribute_info_statement(target.key(), attr_iri.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_attribute_summary).transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::discovery::attribute_discovery::{attribute_info_statement, attribute_summary_statement};
    use andromeda_context_space::context_space_key::ContextSpaceKey;
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;

    fn context_space_key() -> ContextSpaceKey {
        ContextSpaceKey::try_from(1).expect("positive context-space key")
    }

    #[test]
    fn the_paginated_summary_and_the_single_lookup_share_one_aggregate() {
        let (summary, _) = attribute_summary_statement(context_space_key(), None, 10).build_sqlx(PostgresQueryBuilder);
        let (info, _) = attribute_info_statement(context_space_key(), "https://example.org/speed").build_sqlx(PostgresQueryBuilder);
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
        let (summary, _) = attribute_summary_statement(context_space_key(), None, 10).build_sqlx(PostgresQueryBuilder);
        assert!(
            summary.contains(r#"array_agg(DISTINCT "at"."value") FILTER (WHERE "at"."value" IS NOT NULL)"#),
            "{summary}"
        );
    }

    /// JSON-LD 1.1 section 4.1 spells a node object's `@type` either as a string
    /// or as an array of strings, so the expansion is guarded by `jsonb_typeof`
    /// rather than assuming the array spelling: the bare string is lifted into
    /// the array it stands for, and any other JSON type names no type at all.
    /// Without the guard one such node raises `cannot extract elements from a
    /// scalar` and fails the whole aggregate.
    #[test]
    fn the_attribute_type_expansion_reads_both_json_ld_spellings_of_at_type() {
        for (statement, _) in [
            attribute_summary_statement(context_space_key(), None, 10).build_sqlx(PostgresQueryBuilder),
            attribute_info_statement(context_space_key(), "https://example.org/speed").build_sqlx(PostgresQueryBuilder),
        ] {
            assert!(statement.contains("jsonb_array_elements_text((CASE WHEN"), "{statement}");
            assert!(statement.contains("jsonb_build_array"), "{statement}");
            assert!(
                statement.matches(r#"jsonb_typeof(("attrs"."attr_value" -> $"#).count() == 2,
                "both spellings are tested before the expansion runs: {statement}"
            );
            assert!(statement.contains("::jsonb"), "the fallback is an empty JSON array: {statement}");
        }
    }

    #[test]
    fn the_summary_is_keyset_paged_on_the_attribute_iri() {
        let (summary, values) = attribute_summary_statement(context_space_key(), Some("https://example.org/a"), 10).build_sqlx(PostgresQueryBuilder);
        assert!(summary.contains(r#"HAVING ($19::text) IS NULL OR "attrs"."attr_key" > $20"#), "{summary}");
        assert!(summary.contains(r#"ORDER BY "attrs"."attr_key" ASC"#), "{summary}");
        assert!(summary.contains("LIMIT $21"), "{summary}");
        // Every literal the statement carries binds rather than interpolating —
        // the keyword-skip pattern, the container index and key, the `@type`
        // node read three times over the `CASE` and the two spellings it tests,
        // the join conditions, the cursor (twice) and the limit. Only the empty
        // JSON array the `CASE` falls back to is written inline, as a typed
        // `'[]'::jsonb` constant that carries no request value.
        assert_eq!(values.0.0.len(), 21, "{summary}");
    }

    /// The summary reports type IRIs, never keys, so the unnested keys are
    /// joined to the dictionary before they are aggregated.
    #[test]
    fn the_attribute_summary_aggregates_type_names_through_the_dictionary() {
        let (summary, _) = attribute_summary_statement(context_space_key(), None, 10).build_sqlx(PostgresQueryBuilder);
        assert!(summary.contains(r#"array_agg(DISTINCT "n"."name") AS "entity_types""#), "{summary}");
        assert!(
            summary.contains(r#"INNER JOIN LATERAL (SELECT UNNEST("s"."type_ids") AS "t") AS "t" ON $"#),
            "{summary}"
        );
        assert!(!summary.contains(r#""entities""#), "the identity row is never read: {summary}");
    }
}
