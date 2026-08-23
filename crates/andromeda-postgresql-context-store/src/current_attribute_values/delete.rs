//! Removing projection rows an entity mutation no longer stands behind.

use crate::{current_attribute_values::row::ProjectedAttributeKey, error::ContextStoreError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_array::any,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{Expr, ExprTrait, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Postgres, Transaction};

/// The `UNNEST` set-delete's staging alias, `d`.
///
/// The named instances are read under this alias inside the correlated
/// existence test; it is spelled nowhere else, so the single `AS "d"` the table
/// source emits is the one [`apply_column_alias`] rewrites.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct Staging;

/// Delete the projection rows of the named attribute instances.
///
/// Deleting an attribute instance (clause 5.6.13) or an attribute in full
/// (clause 5.6.12, which addresses every instance of it) removes the row rather
/// than marking it: the projection answers what an entity holds now, and an
/// instance that is gone holds nothing. The temporal evolution keeps its own
/// record of the deletion and is untouched by this.
///
/// Runs in the caller's transaction, so the removal and the entity mutation
/// that ordered it commit together.
///
/// The `dataset_id` array carries NULLs (`Vec<Option<&str>>`), which
/// `sea-query-sqlx` cannot represent, so the three column arrays bind through
/// `sqlx` in the marker order [`delete_instances_sql`] emits; the statement
/// structure is still built by sea-query.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn delete_instances_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    keys: &[ProjectedAttributeKey],
) -> Result<(), ContextStoreError> {
    if keys.is_empty() {
        return Ok(());
    }

    let mut entity_keys = Vec::with_capacity(keys.len());
    let mut attribute_name_ids = Vec::with_capacity(keys.len());
    let mut dataset_ids = Vec::with_capacity(keys.len());
    for key in keys {
        entity_keys.push(EntityKey::get(key.entity_key));
        attribute_name_ids.push(AttributeNameId::get(key.attribute_name_id));
        dataset_ids.push(key.dataset_id.as_ref().map(Uri::as_str));
    }
    let (entity_keys, attribute_name_ids, dataset_ids) = (entity_keys, attribute_name_ids, dataset_ids);

    let sql = delete_instances_sql();
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&entity_keys)
        .bind(&attribute_name_ids)
        .bind(&dataset_ids)
        .bind(target.key().as_i64())
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// Delete every projection row of the named entities.
///
/// Only for a mutation that empties an entity's attributes without removing the
/// entity itself. Deleting the entity needs no call at all: the projection's
/// single foreign key cascades from `entities`, and that one ownership edge is
/// the whole deletion path.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn delete_entity_projections_in_tx(tx: &mut Transaction<'_, Postgres>, entity_keys: &[EntityKey]) -> Result<(), ContextStoreError> {
    if entity_keys.is_empty() {
        return Ok(());
    }

    let keys: Vec<i64> = entity_keys.iter().copied().map(EntityKey::get).collect();
    let (sql, values) = entity_delete_statement(keys).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut **tx).await?;

    Ok(())
}

/// Build the entity-wide delete: `entity_key = ANY($1::bigint[])`.
///
/// The surrogate keys bind through sea-query as one array, since none is NULL,
/// rather than one placeholder per key — a batch cannot exhaust the wire
/// protocol's parameter space that way.
fn entity_delete_statement(keys: Vec<i64>) -> sea_query::DeleteStatement {
    Query::delete()
        .from_table(CurrentAttributeValues::Table)
        .and_where(Expr::col(CurrentAttributeValues::EntityKey).eq(any(cast(Expr::val(keys), PostgresType::BigintArray))))
        .to_owned()
}

/// The three staging columns of the instance delete, in bind order.
fn staging_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(CurrentAttributeValues::EntityKey, PostgresType::BigintArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeNameId, PostgresType::IntegerArray),
        UnnestColumn::new(CurrentAttributeValues::DatasetId, PostgresType::TextArray),
    ]
}

/// Build the instance-delete statement.
///
/// The `datasetId` comparison is `IS NOT DISTINCT FROM` rather than `=`, because
/// the default instance is the one carrying no `datasetId` at all (clause 4.5.5)
/// and an equality test against a null matches nothing — the default instance
/// would survive every delete aimed at it.
///
/// `DELETE … USING` is expressed as a correlated `EXISTS`: sea-query has no
/// `USING` clause, and the existence test deletes exactly the rows the staged
/// grain names, which is what the join did.
fn delete_instances_sql() -> String {
    let columns = staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let mut staged = Query::select();
    staged
        .column((Staging, CurrentAttributeValues::EntityKey))
        .from_function(call, Staging)
        .and_where(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::EntityKey)).equals((Staging, CurrentAttributeValues::EntityKey)))
        .and_where(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::ContextSpaceKey)).eq(Expr::cust("$4")))
        .and_where(
            Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::AttributeNameId)).equals((Staging, CurrentAttributeValues::AttributeNameId)),
        )
        .and_where(
            SimpleExpr::from(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::DatasetId)))
                .not_distinct_from(Expr::col((Staging, CurrentAttributeValues::DatasetId))),
        );
    let staged = staged;

    let sql = Query::delete()
        .from_table(CurrentAttributeValues::Table)
        .and_where(Expr::exists(staged))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &Staging, &columns)
}

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::delete::{delete_instances_sql, entity_delete_statement};
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;

    #[test]
    fn the_instance_delete_reaches_the_default_instance() {
        assert!(
            delete_instances_sql().contains(r#""current_attribute_values"."dataset_id" IS NOT DISTINCT FROM "d"."dataset_id""#),
            "an equality test would never match the null datasetId of a default instance: {}",
            delete_instances_sql()
        );
    }

    #[test]
    fn the_instance_delete_is_a_correlated_existence_test_over_the_staged_grain() {
        let sql = delete_instances_sql();
        assert!(sql.contains(r#"DELETE FROM "current_attribute_values" WHERE EXISTS"#), "{sql}");
        assert!(sql.contains(r#""current_attribute_values"."entity_key" = "d"."entity_key""#), "{sql}");
        assert!(
            sql.contains(r#""current_attribute_values"."attribute_name_id" = "d"."attribute_name_id""#),
            "{sql}"
        );
    }

    #[test]
    fn the_instance_delete_binds_one_contiguous_array_per_grain_column() {
        let sql = delete_instances_sql();
        for cast in ["$1::bigint[]", "$2::integer[]", "$3::text[]"] {
            assert!(sql.contains(cast), "missing {cast} in {sql}");
        }
        assert!(sql.contains("AS d(entity_key, attribute_name_id, dataset_id)"), "{sql}");
        assert!(!sql.contains("$4"), "only the three grain arrays are bound: {sql}");
    }

    #[test]
    fn the_entity_delete_addresses_the_surrogate_key_alone() {
        let (sql, values) = entity_delete_statement(vec![1, 2]).build_sqlx(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"DELETE FROM "current_attribute_values" WHERE "entity_key" = ANY($1::bigint[])"#),
            "{sql}"
        );
        assert_eq!(values.0.0.len(), 1, "the whole key array is one bound value: {sql}");
    }
}
