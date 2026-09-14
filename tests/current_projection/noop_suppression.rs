//! Verification-matrix row "Current projection no-op suppression".
//!
//! The projection write carries an `IS DISTINCT FROM` guard over every
//! non-grain column, so a batch that restates an instance with the value it
//! already holds must not rewrite the row: no new heap tuple, no index tuples.
//! The broker's own statement is driven here inside one transaction, and the
//! transaction-local update counter of the relation is read after each batch,
//! which counts exactly the rows the conflict action rewrote and nothing else.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, entity_key::EntityKey};
use andromeda_postgresql_context_store::current_attribute_values::{
    row::{ProjectedAttributeKey, ProjectedAttributeRow, ProjectedAttributeValue},
    upsert::upsert_projection_in_tx,
};
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use pretty_assertions::assert_eq;
use sqlx::{Postgres, Transaction};

/// The rows the relation's update path has rewritten in this transaction.
async fn rows_updated_in_transaction(transaction: &mut Transaction<'_, Postgres>) -> i64 {
    sqlx::query_scalar("SELECT pg_stat_get_xact_tuples_updated('current_attribute_values'::regclass)")
        .fetch_one(&mut **transaction)
        .await
        .expect("the transaction-local update counter must be readable")
}

#[tokio::test]
async fn current_projection_restating_an_identical_value_rewrites_no_row() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let id = scope.id("Building:noop-suppression");
    let mut transaction = scope.database_pool().begin().await.expect("the transaction must open");
    // The live branch is the default schema; the projection upsert names no branch.
    let target = ContextTarget::Live;

    let entity_key: i64 = sqlx::query_scalar("INSERT INTO entities (id) VALUES ($1) RETURNING entity_key")
        .bind(&id)
        .fetch_one(&mut *transaction)
        .await
        .expect("the entity row must be writable");
    let attribute_name_id: i32 = sqlx::query_scalar(
        "INSERT INTO attribute_names (name) VALUES ('https://example.test/current-projection/noop') \
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
    )
    .fetch_one(&mut *transaction)
    .await
    .expect("the attribute dictionary must be writable");
    let unit_code: i16 = sqlx::query_scalar("SELECT min(id) FROM unit_codes")
        .fetch_one(&mut *transaction)
        .await
        .expect("the unit catalogue must be seeded");

    let row = |value: &str, unit_code_id: Option<UnitCodeId>| ProjectedAttributeRow {
        key: ProjectedAttributeKey {
            entity_key: EntityKey::try_from(entity_key).expect("a positive surrogate key"),
            attribute_name_id: attribute_name_id.into(),
            dataset_id: None,
        },
        attribute_kind: AttributeKind::Property,
        value: ProjectedAttributeValue::String(value.to_owned()),
        unit_code_id,
    };

    // The first write inserts; the identical restatement matches and its guard
    // must keep the direct update from touching the row.
    upsert_projection_in_tx(&mut transaction, &target, &[row("reading-1", None)])
        .await
        .expect("the first projection write must land");
    upsert_projection_in_tx(&mut transaction, &target, &[row("reading-1", None)])
        .await
        .expect("the identical restatement must succeed");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        0,
        "an identical restatement must rewrite no row"
    );

    // A changed value is a genuine update.
    upsert_projection_in_tx(&mut transaction, &target, &[row("reading-2", None)])
        .await
        .expect("the changed value must land");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        1,
        "a changed value must rewrite the row once"
    );

    // A change confined to the unit is a difference in a non-grain column too.
    let unit = UnitCodeId::try_from(unit_code).expect("a catalogued unit identifier");
    upsert_projection_in_tx(&mut transaction, &target, &[row("reading-2", Some(unit))])
        .await
        .expect("the unit change must land");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        2,
        "a unit-only change must rewrite the row once more"
    );

    let (stored_value, stored_unit): (Option<String>, Option<i16>) =
        sqlx::query_as("SELECT value_string, unit_code_id FROM current_attribute_values WHERE entity_key = $1")
            .bind(entity_key)
            .fetch_one(&mut *transaction)
            .await
            .expect("the projected row must be readable");
    assert_eq!((stored_value.as_deref(), stored_unit), (Some("reading-2"), Some(unit_code)));

    transaction.rollback().await.expect("the probe transaction must roll back");
    scope.finish().await.expect("cleanup failed");
}
