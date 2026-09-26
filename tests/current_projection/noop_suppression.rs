//! Verification-matrix row "Current projection no-op suppression".
//!
//! The projection write carries an `IS DISTINCT FROM` guard over every
//! non-grain column, so a batch that restates an instance with the value it
//! already holds must not rewrite the row: no new heap tuple, no index tuples.
//! The broker's own statement is driven here inside one transaction, and the
//! transaction-local update counter of the relation is read after each batch,
//! which counts exactly the rows the conflict action rewrote and nothing else.

use crate::bypass_writes::insert_attribute_name;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, entity_key::EntityKey};
use andromeda_postgresql_context_store::current_attribute_values::{
    row::{ProjectedAttributeKey, ProjectedAttributeRow, ProjectedAttributeValue},
    upsert::upsert_projection_in_tx,
};
use andromeda_test_fixtures::high_entropy_text::high_entropy_text;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use pretty_assertions::assert_eq;
use sqlx::{Postgres, Transaction};

/// Length of the long strings the overflow-column case restates, past the
/// indexed bound.
const LONG_TEXT_LENGTH: usize = 6144;

/// One projection grain opened inside a probe transaction: a bare Entity row
/// and a dictionary attribute name, with no projection row yet.
struct ProbeGrain {
    /// Surrogate key of the bare Entity row.
    entity_key: i64,
    /// Dictionary identifier of the probed attribute.
    attribute_name_id: i32,
}

impl ProbeGrain {
    /// Insert the bare Entity row and register the attribute name.
    async fn open(transaction: &mut Transaction<'_, Postgres>, entity_id: &str, attribute_iri: &str) -> ProbeGrain {
        let entity_key: i64 = sqlx::query_scalar("INSERT INTO entities (id) VALUES ($1) RETURNING entity_key")
            .bind(entity_id)
            .fetch_one(&mut **transaction)
            .await
            .expect("the entity row must be writable");
        let attribute_name_id = insert_attribute_name(transaction, attribute_iri).await;
        ProbeGrain { entity_key, attribute_name_id }
    }

    /// The default-instance projection row holding one String value.
    fn string_row(&self, value: &str, unit_code_id: Option<UnitCodeId>) -> ProjectedAttributeRow {
        ProjectedAttributeRow {
            key: ProjectedAttributeKey {
                entity_key: EntityKey::try_from(self.entity_key).expect("a positive surrogate key"),
                attribute_name_id: self.attribute_name_id.into(),
                dataset_id: None,
            },
            attribute_kind: AttributeKind::Property,
            value: ProjectedAttributeValue::String(value.to_owned()),
            unit_code_id,
        }
    }
}

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

    let grain = ProbeGrain::open(&mut transaction, &id, "https://example.test/current-projection/noop").await;
    let unit_code: i16 = sqlx::query_scalar("SELECT min(id) FROM unit_codes")
        .fetch_one(&mut *transaction)
        .await
        .expect("the unit catalogue must be seeded");

    // The first write inserts; the identical restatement matches and its guard
    // must keep the direct update from touching the row.
    upsert_projection_in_tx(&mut transaction, &target, &[grain.string_row("reading-1", None)])
        .await
        .expect("the first projection write must land");
    upsert_projection_in_tx(&mut transaction, &target, &[grain.string_row("reading-1", None)])
        .await
        .expect("the identical restatement must succeed");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        0,
        "an identical restatement must rewrite no row"
    );

    // A changed value is a genuine update.
    upsert_projection_in_tx(&mut transaction, &target, &[grain.string_row("reading-2", None)])
        .await
        .expect("the changed value must land");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        1,
        "a changed value must rewrite the row once"
    );

    // A change confined to the unit is a difference in a non-grain column too.
    let unit = UnitCodeId::try_from(unit_code).expect("a catalogued unit identifier");
    upsert_projection_in_tx(&mut transaction, &target, &[grain.string_row("reading-2", Some(unit))])
        .await
        .expect("the unit change must land");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        2,
        "a unit-only change must rewrite the row once more"
    );

    let (stored_value, stored_unit): (Option<String>, Option<i16>) =
        sqlx::query_as("SELECT value_string, unit_code_id FROM current_attribute_values WHERE entity_key = $1")
            .bind(grain.entity_key)
            .fetch_one(&mut *transaction)
            .await
            .expect("the projected row must be readable");
    assert_eq!((stored_value.as_deref(), stored_unit), (Some("reading-2"), Some(unit_code)));

    transaction.rollback().await.expect("the probe transaction must roll back");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_restating_an_identical_long_string_rewrites_no_row() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let id = scope.id("Building:noop-suppression-long");
    let mut transaction = scope.database_pool().begin().await.expect("the transaction must open");
    // The live branch is the default schema; the projection upsert names no branch.
    let target = ContextTarget::Live;
    let grain = ProbeGrain::open(&mut transaction, &id, "https://example.test/current-projection/noop-long").await;
    let first = high_entropy_text(LONG_TEXT_LENGTH, 41);
    let second = high_entropy_text(LONG_TEXT_LENGTH, 42);

    // The guard compares the overflow column like any other non-grain column,
    // so a restated long value is as free as a restated short one.
    upsert_projection_in_tx(&mut transaction, &target, &[grain.string_row(&first, None)])
        .await
        .expect("the first projection write must land");
    upsert_projection_in_tx(&mut transaction, &target, &[grain.string_row(&first, None)])
        .await
        .expect("the identical restatement must succeed");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        0,
        "an identical long restatement must rewrite no row"
    );

    upsert_projection_in_tx(&mut transaction, &target, &[grain.string_row(&second, None)])
        .await
        .expect("the changed long value must land");
    assert_eq!(
        rows_updated_in_transaction(&mut transaction).await,
        1,
        "a changed long value must rewrite the row once"
    );

    let stored: (Option<String>, Option<String>) = sqlx::query_as("SELECT value_string, value_long_text FROM current_attribute_values WHERE entity_key = $1")
        .bind(grain.entity_key)
        .fetch_one(&mut *transaction)
        .await
        .expect("the projected row must be readable");
    assert_eq!(stored, (None, Some(second)), "the long value must sit in the overflow column");

    transaction.rollback().await.expect("the probe transaction must roll back");
    scope.finish().await.expect("cleanup failed");
}
