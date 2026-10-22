//! Schema contract checks for the durable temporal append queue.

use crate::SUITE;
use andromeda_postgresql_seaquery::{
    aggregate::{array_agg_ordered, count_all},
    cast::cast,
    introspection::current_schema,
    literal::text_literal,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::{InformationSchema, InformationSchemaColumns},
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Query};

/// The queue column must carry the same `write_id` name every runtime
/// statement that inserts, acknowledges and retries an intent uses, and the
/// three-part idempotency constraint must be attached to that identity. The
/// name `intent_id` must not exist at all. The queue
/// has no dead-letter state -- an intent is the durable record of accepted
/// history and stays claimable however often it fails -- so no terminal
/// column is expected here either; the failure summary it keeps is bounded by
/// the `last_error` length check.
#[tokio::test]
async fn temporal_append_intents_exposes_write_identity_and_preserves_its_deduplication_key() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let queue_columns = || {
        Query::select()
            .from((InformationSchema, InformationSchemaColumns::Table))
            .and_where(Expr::col(InformationSchemaColumns::TableSchema).eq(current_schema()))
            .and_where(Expr::col(InformationSchemaColumns::TableName).eq("temporal_append_intents"))
            .to_owned()
    };
    let write_id: (String, String) = queue_columns()
        .columns([Alias::new("data_type"), Alias::new("is_nullable")])
        .and_where(Expr::col(InformationSchemaColumns::ColumnName).eq("write_id"))
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the temporal append queue must expose write_id");
    assert_eq!(write_id, ("uuid".to_owned(), "NO".to_owned()));

    let legacy_column: i64 = queue_columns()
        .expr(count_all())
        .and_where(Expr::col(InformationSchemaColumns::ColumnName).eq("intent_id"))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the temporal append queue columns must be readable");
    assert_eq!(legacy_column, 0, "the superseded intent_id column name must not exist");

    let constraint = |column: &str| Expr::col((Alias::new("constraint_definition"), Alias::new(column)));
    let key_columns = [UnnestColumn::new(Alias::new("attnum"), PostgresType::SmallintArray)];
    let constraint_columns: Vec<String> = UnnestAliased::new(
        Query::select()
            .expr(cast(
                array_agg_ordered(
                    Expr::col((Alias::new("attribute"), Alias::new("attname"))),
                    Expr::col((Alias::new("key"), Alias::new("ordinality"))),
                ),
                PostgresType::TextArray,
            ))
            .from_as(Alias::new("pg_constraint"), Alias::new("constraint_definition"))
            .from_function(unnest_call([(&key_columns[0], constraint("conkey"))]), Alias::new("key"))
            .from_as(Alias::new("pg_attribute"), Alias::new("attribute"))
            .and_where(Expr::col((Alias::new("attribute"), Alias::new("attrelid"))).equals((Alias::new("constraint_definition"), Alias::new("conrelid"))))
            .and_where(Expr::col((Alias::new("attribute"), Alias::new("attnum"))).equals((Alias::new("key"), Alias::new("attnum"))))
            .and_where(constraint("conrelid").eq(cast(text_literal("temporal_append_intents"), PostgresType::Regclass)))
            .and_where(constraint("conname").eq("uq_temporal_append_intents_write_identity"))
            .group_by_col((Alias::new("constraint_definition"), Alias::new("oid")))
            .to_owned(),
    )
    .with_ordinality(Alias::new("key"), &key_columns, Alias::new("ordinality"))
    .to_query_scalar()
    .fetch_one(pool)
    .await
    .expect("the temporal append intent uniqueness constraint must exist");
    assert_eq!(constraint_columns, ["write_id", "entity_id", "base_ordinal"]);

    let bounded_error: i64 = Query::select()
        .expr(count_all())
        .from(Alias::new("pg_constraint"))
        .and_where(Expr::col(Alias::new("conrelid")).eq(cast(text_literal("temporal_append_intents"), PostgresType::Regclass)))
        .and_where(Expr::col(Alias::new("conname")).eq("chk_temporal_append_intents_last_error_length"))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the queue constraints must be readable");
    assert_eq!(bounded_error, 1, "the failure summary the queue keeps must be length-bounded");

    let terminal_columns: i64 = queue_columns()
        .expr(count_all())
        .and_where(Expr::col(InformationSchemaColumns::ColumnName).is_in(["dead", "exhausted_at", "dead_lettered_at"]))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the temporal append queue columns must be readable");
    assert_eq!(
        terminal_columns, 0,
        "an intent has no dead-letter state: it stays claimable however often it fails"
    );

    scope.finish().await.expect("cleanup failed");
}
