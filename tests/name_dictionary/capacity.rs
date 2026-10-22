//! Attribute dictionary capacity: the key space is not bounded by `SMALLINT`.
//!
//! The dictionary key is `INTEGER` precisely so that a tenant modelling an open world is not stopped at the
//! 32,767 entries a `SMALLINT` key could number. The check is therefore made
//! past that boundary: 32,768 distinct valid attribute IRIs are interned, and
//! every one of them keeps a positive, distinct key that the same array-bound
//! lookup the broker issues reads back unchanged.

use crate::{generated_names_insert, run_name_prefix};
use andromeda_postgresql_schema::name_dictionary::AttributeNames;
use andromeda_postgresql_seaquery::{
    aggregate::{count_all, count_distinct},
    cast::cast,
    pg_array::any,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::{InformationSchema, InformationSchemaColumns},
};
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Func, Query};
use sqlx::Row;

/// How many distinct names the capacity check interns.
///
/// One past the largest key a `SMALLINT` could hold.
const GENERATED_NAMES: i32 = 32_768;

/// Names asked for in one array-bound lookup, the shape the broker's dictionary
/// cache issues when it resolves a batch of previously unseen attributes.
const LOOKUP_BATCH: i32 = 1_000;

#[tokio::test]
async fn thirty_two_thousand_distinct_attribute_names_keep_positive_distinct_keys() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration)
        .await
        .expect("exclusive ownership of the shared dictionary");
    let pool = connect_test_database().await.expect("test database connection");

    let prefix = run_name_prefix();
    let interned = generated_names_insert(AttributeNames::Table, AttributeNames::Name, &prefix, GENERATED_NAMES)
        .to_query()
        .execute(&pool)
        .await
        .expect("interning distinct attribute names must not fail");
    assert_eq!(
        i64::try_from(interned.rows_affected()).expect("a row count fits a signed integer"),
        i64::from(GENERATED_NAMES),
        "every generated name is distinct, so every one of them is a new dictionary entry"
    );

    let row = Query::select()
        .expr_as(count_all(), Alias::new("interned"))
        .expr_as(count_distinct(Expr::col(AttributeNames::AttributeNameKey)), Alias::new("distinct_keys"))
        .expr_as(Func::min(Expr::col(AttributeNames::AttributeNameKey)), Alias::new("lowest"))
        .expr_as(Func::max(Expr::col(AttributeNames::AttributeNameKey)), Alias::new("highest"))
        .from(AttributeNames::Table)
        .and_where(Expr::col(AttributeNames::Name).like(format!("{prefix}%")))
        .to_query()
        .fetch_one(&pool)
        .await
        .expect("the dictionary must be readable");
    let interned_rows: i64 = row.try_get("interned").expect("the interned column decodes");
    let distinct_keys: i64 = row.try_get("distinct_keys").expect("the distinct_keys column decodes");
    let lowest: i32 = row.try_get("lowest").expect("the lowest column decodes");
    let highest: i32 = row.try_get("highest").expect("the highest column decodes");

    assert_eq!(interned_rows, i64::from(GENERATED_NAMES), "one row per distinct name");
    assert_eq!(distinct_keys, interned_rows, "no two names may share a key");
    assert!(lowest > 0, "a dictionary key is positive, found {lowest}");
    assert!(
        highest > i32::from(i16::MAX),
        "the check has to reach past the two-byte ceiling to mean anything, highest key was {highest}"
    );

    // The lookup shape the broker's dictionary cache uses: one array bind
    // rather than a placeholder per name, which is what keeps a large batch
    // from exhausting the protocol's parameter space.
    let batch: Vec<String> = (1..=LOOKUP_BATCH).map(|position| format!("{prefix}{position}")).collect();
    let requested = batch.len();
    let resolved = Query::select()
        .columns([AttributeNames::AttributeNameKey, AttributeNames::Name])
        .from(AttributeNames::Table)
        .and_where(Expr::col(AttributeNames::Name).eq(any(Expr::val(batch))))
        .to_query()
        .fetch_all(&pool)
        .await
        .expect("an array-bound lookup of a large batch must not fail");
    assert_eq!(resolved.len(), requested, "every requested name resolves to exactly one key");
    for resolved_row in &resolved {
        let key: i32 = resolved_row.try_get("attribute_name_key").expect("the attribute_name_key column decodes");
        let name: String = resolved_row.try_get("name").expect("the name column decodes");
        assert!(key > 0, "{name} resolved to a non-positive key");
    }

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

#[tokio::test]
async fn the_dictionary_key_is_declared_wide_enough_to_number_more_than_a_two_byte_key() {
    let pool = connect_test_database().await.expect("test database connection");

    let declared: String = Query::select()
        .expr(cast(Expr::col(Alias::new("data_type")), PostgresType::Text))
        .from((InformationSchema, InformationSchemaColumns::Table))
        .and_where(Expr::col(InformationSchemaColumns::TableSchema).eq("public"))
        .and_where(Expr::col(InformationSchemaColumns::TableName).eq("attribute_names"))
        .and_where(Expr::col(InformationSchemaColumns::ColumnName).eq("attribute_name_key"))
        .to_query_scalar()
        .fetch_one(&pool)
        .await
        .expect("the dictionary key column must be declared");
    assert_eq!(declared, "integer", "a two-byte key would cap the dictionary at 32,767 entries again");

    pool.close().await;
}
