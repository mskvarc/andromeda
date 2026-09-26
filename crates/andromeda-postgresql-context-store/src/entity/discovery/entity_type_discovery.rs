//! The entity-type half of discovery (clauses 5.7.5 to 5.7.7).
//!
//! Three answers are built here: the types at least one entity carries, the
//! attribute keys each of those types is seen with, and how many entities carry
//! one type. All three report dictionary names rather than interned keys, so
//! each of them joins the type dictionary before it aggregates.

use crate::{
    entity::{
        discovery::{
            aggregate_source::{AttrKey, D, attribute_container, keyset_after, search_with_documents},
            type_dictionary::{N, join_type_names, type_name, unnest_entity_type_name_keys},
        },
        mapper::{stored_attribute_name, stored_type_name},
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    current::{EntityDocuments, EntitySearch},
    name_dictionary::EntityTypeNames,
};
use andromeda_postgresql_seaquery::{
    aggregate::{array_agg_distinct_filter, count_all},
    jsonb::jsonb_object_keys,
    operator::{array_contains, array_contains_key},
    pg_array::text_array,
    pg_type::PostgresType,
};
use sea_query::{Expr, ExprTrait, Func, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::Row;

/// The lateral expansion of an entity's attribute keys, aliased `ak`.
#[derive(Iden, Clone, Copy)]
#[iden = "ak"]
struct Ak;

/// The aggregated type IRI a type summary reports.
#[derive(Iden, Clone, Copy)]
#[iden = "type_iri"]
struct TypeIri;

/// The aggregated attribute-key array a type summary reports.
#[derive(Iden, Clone, Copy)]
#[iden = "attr_keys"]
struct AttrKeys;

/// Every dictionary type at least one entity carries, in name order:
/// `SELECT n.name FROM entity_type_names n WHERE EXISTS (SELECT 1 FROM entity_search WHERE entity_type_name_keys @> ARRAY(SELECT n.entity_type_name_key::integer)) ORDER BY n.name`.
fn types_in_use_statement(target: &ContextTarget) -> SelectStatement {
    let mut carried = Query::select();
    carried.expr(Expr::val(1)).from(table_of(target, EntitySearch::Table)).and_where(array_contains(
        Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys)),
        Expr::col((N, EntityTypeNames::EntityTypeNameKey)),
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

/// Build the per-type attribute-key summary, keyset-paged on the type IRI.
///
/// Each entity contributes one row per type it carries and per top-level
/// non-keyword attribute key it holds; the aggregate collects the distinct
/// attribute keys of each type into an array, empty when a type has none.
fn entity_type_summaries_statement(target: &ContextTarget, after: Option<&str>, limit: u64) -> SelectStatement {
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
    search_with_documents(&mut statement, target);
    unnest_entity_type_name_keys(&mut statement);
    join_type_names(&mut statement)
        .join_lateral(JoinType::InnerJoin, keys, Ak, SimpleExpr::from(Expr::val(true)))
        .and_where(Expr::col((Ak, AttrKey)).not_like("@%"))
        .group_by_col((N, EntityTypeNames::Name))
        .cond_having(keyset_after(type_name(), after))
        .order_by((N, EntityTypeNames::Name), Order::Asc)
        .limit(limit);
    statement
}

impl EntityRepository {
    /// Return all distinct entity type IRIs present in the store.
    ///
    /// Driven from the dictionary rather than by unnesting every entity's
    /// `entity_type_name_keys`: a type is reported when at least one entity carries its
    /// key, which the `entity_type_name_keys` GIN index answers per dictionary row.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the aggregation fails, or when a
    /// dictionary row holds text that is no longer a valid entity type name.
    pub async fn types(&self, target: &ContextTarget) -> Result<Vec<EntityTypeName>, ContextStoreError> {
        let (sql, values) = types_in_use_statement(target).build_sqlx(PostgresQueryBuilder);
        let raw: Vec<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        raw.iter().map(String::as_str).map(stored_type_name).collect()
    }

    /// Count entities that have the given type IRI.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the dictionary lookup for the type or
    /// the count fails.
    pub async fn count_by_type(&self, target: &ContextTarget, entity_type: &EntityTypeName) -> Result<i64, ContextStoreError> {
        let Some(type_key) = self.type_key(entity_type).await? else {
            return Ok(0);
        };
        let statement = Query::select()
            .expr(count_all())
            .from(table_of(target, EntitySearch::Table))
            .and_where(array_contains_key(Expr::col(EntitySearch::EntityTypeNameKeys), type_key.get()))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(&self.pool).await?)
    }

    /// Per-type attribute summaries, keyset-paginated on the aggregated type IRI.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the aggregation fails, or when an
    /// aggregated type or attribute IRI is no longer a valid lexicon name.
    pub async fn get_entity_type_summaries(
        &self,
        target: &ContextTarget,
        after: Option<&EntityTypeName>,
        limit: u32,
    ) -> Result<Vec<(EntityTypeName, Vec<AttributeName>)>, ContextStoreError> {
        let (sql, values) = entity_type_summaries_statement(target, after.map(EntityTypeName::as_str), u64::from(limit)).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

        rows.into_iter()
            .map(|row| {
                let type_iri = stored_type_name(row.get("type_iri"))?;
                let attr_keys: Vec<String> = row.get("attr_keys");
                let attr_names = attr_keys.iter().map(String::as_str).map(stored_attribute_name).collect::<Result<_, _>>()?;
                Ok((type_iri, attr_names))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::discovery::entity_type_discovery::{entity_type_summaries_statement, types_in_use_statement};
    use andromeda_context_space::context_target::ContextTarget;
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;

    #[test]
    fn the_type_summary_walks_the_keys_laterally_and_coalesces_to_an_empty_array() {
        let (sql, _) = entity_type_summaries_statement(&ContextTarget::Live, None, 10).build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_object_keys"), "{sql}");
        assert!(sql.contains("JOIN LATERAL"), "{sql}");
        assert!(
            sql.contains(r#"COALESCE(array_agg(DISTINCT "ak"."attr_key") FILTER (WHERE "ak"."attr_key" IS NOT NULL)"#),
            "{sql}"
        );
        assert!(sql.contains(r#"GROUP BY "n"."name""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN LATERAL (SELECT UNNEST("s"."entity_type_name_keys") AS "t") AS "t" ON $"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "entity_type_names" AS "n" ON "n"."entity_type_name_key" = "t"."t""#),
            "every unnested type key is read back through the dictionary: {sql}"
        );
    }

    /// The type list is driven from the dictionary and filtered by an
    /// index-served containment probe, so it never unnests the whole table.
    #[test]
    fn the_types_in_use_are_the_dictionary_rows_some_entity_carries() {
        let sql = types_in_use_statement(&ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""entity_search"."entity_type_name_keys" @> ARRAY(SELECT "n"."entity_type_name_key"::integer)"#),
            "{sql}"
        );
    }
}
