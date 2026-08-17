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
use serde_json::{Value, json};

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
async fn current_projection_and_indexed_queries_preserve_neighboring_exact_decimals() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let exact_id = scope.id("Building:atomicity-exact-number");
    let adjacent_id = scope.id("Building:atomicity-adjacent-number");

    let entity: Value = serde_json::from_str(&format!(
        r#"{{"id":"{exact_id}","type":"Building","reading":{{"type":"Property","value":0.100000000000000005}},"large":{{"type":"Property","value":18446744073709551616}},"@context":["{TEST_SUITE_CONTEXT}"]}}"#
    ))
    .expect("arbitrary-precision entity JSON");
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let adjacent: Value = serde_json::from_str(&format!(
        r#"{{"id":"{adjacent_id}","type":"Building","reading":{{"type":"Property","value":0.100000000000000004}},"@context":["{TEST_SUITE_CONTEXT}"]}}"#
    ))
    .expect("adjacent arbitrary-precision entity JSON");
    let created = client.post_entity(&adjacent).send().await.expect("create adjacent entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create adjacent: {:?}", created.body);

    let reading_rows = rows_for_attribute(scope.database_pool(), &exact_id, "reading").await;
    assert_eq!(reading_rows.first().and_then(|row| row.value_text.as_deref()), Some("0.100000000000000005"));
    let large_rows = rows_for_attribute(scope.database_pool(), &exact_id, "large").await;
    assert_eq!(
        large_rows.first().and_then(|row| row.value_text.as_deref()),
        Some("18446744073709551616"),
        "the NUMERIC projection column must hold an integer beyond u64 without rounding"
    );

    let stored_payload: String = sqlx::query_scalar("SELECT payload::text FROM entities WHERE id = $1")
        .bind(&exact_id)
        .fetch_one(scope.database_pool())
        .await
        .expect("stored payload must be readable");
    assert!(stored_payload.contains("0.100000000000000005"), "stored payload: {stored_payload}");
    assert!(stored_payload.contains("18446744073709551616"), "stored payload: {stored_payload}");

    let retrieved = client
        .get_entity(&exact_id, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let retrieved = String::from_utf8(retrieved.body.to_vec()).expect("response JSON is UTF-8");
    assert!(retrieved.contains("0.100000000000000005"), "retrieved entity: {retrieved}");
    assert!(retrieved.contains("18446744073709551616"), "retrieved entity: {retrieved}");

    let cases = [
        ("reading==0.100000000000000005", vec![exact_id.as_str()]),
        ("reading!=0.100000000000000005", vec![adjacent_id.as_str()]),
        ("reading<0.100000000000000005", vec![adjacent_id.as_str()]),
        ("reading>0.100000000000000004", vec![exact_id.as_str()]),
        (
            "reading==0.100000000000000004..0.100000000000000005",
            vec![adjacent_id.as_str(), exact_id.as_str()],
        ),
        (
            "reading==0.100000000000000004,0.100000000000000005",
            vec![adjacent_id.as_str(), exact_id.as_str()],
        ),
    ];
    for (q, expected_ids) in cases {
        let response = client
            .query_entities(&format!("type=Building&q={q}"), "application/json", Some(TEST_SUITE_CONTEXT))
            .await
            .expect("exact numeric query failed");
        assert_eq!(response.status, StatusCode::OK, "query {q}: {:?}", response.body);
        let body = response.json_value().expect("query response must be JSON");
        let mut ids = body
            .as_array()
            .expect("query response must be an array")
            .iter()
            .filter_map(|entity| entity["id"].as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        let mut expected_ids = expected_ids;
        expected_ids.sort_unstable();
        assert_eq!(ids, expected_ids, "query {q} must distinguish adjacent decimals");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_rejects_an_out_of_range_number_without_mutation() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:out-of-range-number");
    let entity: Value = serde_json::from_str(&format!(
        r#"{{"id":"{id}","type":"Building","reading":{{"type":"Property","value":1e-16384}},"@context":["{TEST_SUITE_CONTEXT}"]}}"#
    ))
    .expect("syntactically valid JSON number");

    let rejected = client.post_entity(&entity).send().await.expect("create request failed");
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST, "rejection: {:?}", rejected.body);
    let problem = rejected.json_value().expect("problem details must be JSON");
    assert!(
        problem["type"].as_str().is_some_and(|value| value.contains("BadRequestData")),
        "problem details: {problem}"
    );

    let entity_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM entities WHERE id = $1")
        .bind(&id)
        .fetch_one(scope.database_pool())
        .await
        .expect("entity count must be readable");
    assert_eq!(entity_rows, 0, "the rejected entity must not be persisted");
    assert!(
        rows_for_attribute(scope.database_pool(), &id, "reading").await.is_empty(),
        "the rejected number must not reach the projection"
    );

    scope.finish().await.expect("cleanup failed");
}
