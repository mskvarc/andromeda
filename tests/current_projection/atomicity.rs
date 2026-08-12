//! Verification-matrix row "Current projection atomicity".
//!
//! The projection is written inside the Entity transaction, so a committed
//! write is visible to every later reader with no barrier, and a projection
//! failure takes the Entity row down with it.

use crate::projection_rows::{ProjectionRow, rows_for_attribute};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The expanded Building type of the ETSI test-suite context.
const BUILDING_TYPE: &str = "https://ngsi-ld-test-suite/context#Building";

#[tokio::test]
async fn current_projection_update_is_visible_to_a_new_connection_without_a_barrier() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:atomicity-visible");

    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 21.5},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let fragment = json!({
        "temperature": {"type": "Property", "value": 42.5},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let updated = client.update_entity_attrs(&id, &fragment).await.expect("update attributes failed");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "update: {:?}", updated.body);

    // No sleep and no reconciliation: the very next query on the broker's own
    // pool must already filter on the new value.
    let query = format!("type=Building&id={}&q=temperature==42.5", urlencoding::encode(&id));
    let filtered = client
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query failed");
    assert_eq!(filtered.status, StatusCode::OK);
    let matched = filtered.json_value().expect("query body must be JSON");
    assert_eq!(
        matched.as_array().map(Vec::len),
        Some(1),
        "the committed value must be filterable immediately: {matched:?}"
    );

    // A connection the write never touched observes the same typed projection.
    let rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    assert_eq!(
        rows,
        vec![ProjectionRow {
            attribute_name: "https://uri.etsi.org/ngsi-ld/default-context/temperature".to_owned(),
            dataset_id: None,
            attribute_kind: 1,
            value_kind: 1,
            value_text: Some("42.5".to_owned()),
        }]
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_constraint_violation_rolls_back_the_entity_row() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let id = scope.id("Building:atomicity-rollback");

    let mut transaction = scope.database_pool().begin().await.expect("transaction must start");
    let payload = json!([{ "@id": id, "@type": [BUILDING_TYPE] }]);
    let entity_key: i64 = sqlx::query_scalar("INSERT INTO entities (id, types, payload) VALUES ($1, ARRAY[$2], $3) RETURNING entity_key")
        .bind(&id)
        .bind(BUILDING_TYPE)
        .bind(&payload)
        .fetch_one(&mut *transaction)
        .await
        .expect("entity insert must succeed");

    // A projection row whose declared value kind does not match its populated
    // column: exactly the corruption
    // `chk_current_attribute_values_value_kind_column` exists to refuse.
    let name_id: i32 = sqlx::query_scalar("INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id")
        .bind("https://example.test/current-projection/rollback")
        .fetch_one(&mut *transaction)
        .await
        .expect("dictionary insert must succeed");
    let failure = sqlx::query(
        "INSERT INTO current_attribute_values \
         (entity_key, modified_at, attribute_name_id, attribute_kind, value_kind, value_string) \
         VALUES ($1, now(), $2, 1, 1, 'not a number')",
    )
    .bind(entity_key)
    .bind(name_id)
    .execute(&mut *transaction)
    .await
    .expect_err("a projection row contradicting its value kind must be refused");
    let database_error = failure.as_database_error().expect("the refusal must come from the database");
    assert_eq!(
        database_error.code().as_deref(),
        Some("23514"),
        "the refusal must be a check violation: {database_error}"
    );

    transaction.rollback().await.expect("rollback must succeed");

    let survivors: i64 = sqlx::query_scalar("SELECT count(*) FROM entities WHERE id = $1")
        .bind(&id)
        .fetch_one(scope.database_pool())
        .await
        .expect("entity count must be readable");
    assert_eq!(survivors, 0, "the Entity row must not survive its failed projection write");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_number_column_stores_the_exact_json_number() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:atomicity-exact-number");

    // 2^53 + 1 is the smallest positive integer `double precision` cannot hold.
    // `value_number` is `NUMERIC` precisely so the projection agrees with the
    // payload, which stores the JSON number verbatim.
    let entity = json!({
        "id": id,
        "type": "Building",
        "reading": {"type": "Property", "value": 9_007_199_254_740_993_i64},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let rows = rows_for_attribute(scope.database_pool(), &id, "reading").await;
    assert_eq!(
        rows.first().and_then(|row| row.value_text.as_deref()),
        Some("9007199254740993"),
        "the NUMERIC projection column must not round the value the payload stores exactly"
    );

    scope.finish().await.expect("cleanup failed");
}
