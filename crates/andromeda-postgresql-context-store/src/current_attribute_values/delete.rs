//! Removing projection rows an entity mutation no longer stands behind.

use crate::{current_attribute_values::row::ProjectedAttributeKey, error::ContextStoreError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::{
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{Expr, ExprTrait, Iden, PostgresQueryBuilder, Query, SimpleExpr};
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
    let mut attribute_name_keys = Vec::with_capacity(keys.len());
    let mut dataset_ids = Vec::with_capacity(keys.len());
    for key in keys {
        entity_keys.push(EntityKey::get(key.entity_key));
        attribute_name_keys.push(AttributeNameKey::get(key.attribute_name_key));
        dataset_ids.push(key.dataset_id.as_ref().map(Identifier::as_str));
    }
    let (entity_keys, attribute_name_keys, dataset_ids) = (entity_keys, attribute_name_keys, dataset_ids);

    let sql = delete_instances_sql(target);
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&entity_keys)
        .bind(&attribute_name_keys)
        .bind(&dataset_ids)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// The three staging columns of the instance delete, in bind order.
fn staging_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(CurrentAttributeValues::EntityKey, PostgresType::BigintArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeNameKey, PostgresType::IntegerArray),
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
fn delete_instances_sql(target: &ContextTarget) -> String {
    let columns = staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let mut staged = Query::select();
    staged
        .column((Staging, CurrentAttributeValues::EntityKey))
        .from_function(call, Staging)
        .and_where(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::EntityKey)).equals((Staging, CurrentAttributeValues::EntityKey)))
        .and_where(
            Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::AttributeNameKey)).equals((Staging, CurrentAttributeValues::AttributeNameKey)),
        )
        .and_where(
            SimpleExpr::from(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::DatasetId)))
                .not_distinct_from(Expr::col((Staging, CurrentAttributeValues::DatasetId))),
        );
    let staged = staged;

    let sql = Query::delete()
        .from_table(table_of(target, CurrentAttributeValues::Table))
        .and_where(Expr::exists(staged))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &Staging, &columns)
}

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::delete::delete_instances_sql;
    use andromeda_context_space::context_target::ContextTarget;

    #[test]
    fn the_instance_delete_reaches_the_default_instance() {
        assert!(
            delete_instances_sql(&ContextTarget::Live).contains(r#""current_attribute_values"."dataset_id" IS NOT DISTINCT FROM "d"."dataset_id""#),
            "an equality test would never match the null datasetId of a default instance: {}",
            delete_instances_sql(&ContextTarget::Live)
        );
    }

    #[test]
    fn the_instance_delete_is_a_correlated_existence_test_over_the_staged_grain() {
        let sql = delete_instances_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"DELETE FROM "current_attribute_values" WHERE EXISTS"#), "{sql}");
        assert!(sql.contains(r#""current_attribute_values"."entity_key" = "d"."entity_key""#), "{sql}");
        assert!(
            sql.contains(r#""current_attribute_values"."attribute_name_key" = "d"."attribute_name_key""#),
            "{sql}"
        );
    }

    #[test]
    fn the_instance_delete_binds_one_contiguous_array_per_grain_column() {
        let sql = delete_instances_sql(&ContextTarget::Live);
        for cast in ["$1::bigint[]", "$2::integer[]", "$3::text[]"] {
            assert!(sql.contains(cast), "missing {cast} in {sql}");
        }
        assert!(sql.contains("AS d(entity_key, attribute_name_key, dataset_id)"), "{sql}");
        assert!(!sql.contains("$4"), "only the three grain arrays are bound: {sql}");
    }
}
