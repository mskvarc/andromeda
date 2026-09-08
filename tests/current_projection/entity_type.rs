//! Verification-matrix row "Empty Entity type".
//!
//! An Entity always has at least one type (clause 4.5.1). Rust rejects the
//! empty array at the API boundary, and `chk_entity_search_types_present`
//! refuses it again for any writer that bypasses the API.

use crate::projection_rows::live_context_space_key;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// Expanded Building type of the ETSI test-suite context.
const BUILDING_TYPE: &str = "https://ngsi-ld-test-suite/context#Building";

#[tokio::test]
async fn current_projection_rejects_an_entity_with_an_empty_type_array() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:empty-type");

    let entity = json!({
        "id": id,
        "type": [],
        "temperature": {"type": "Property", "value": 1.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.post_entity(&entity).send().await.expect("create entity request failed");
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "an Entity with no type must be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_database_refuses_a_typeless_entity_row() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let id = scope.id("Building:empty-type-sql");
    let context_space_key = live_context_space_key(scope.database_pool()).await;

    let payload = json!([{ "@id": id }]);
    let failure = sqlx::query(
        "WITH ins AS (INSERT INTO entities (context_space_key, id) VALUES ($1, $2) RETURNING entity_key), \
              doc AS (INSERT INTO entity_documents (context_space_key, entity_key, payload) SELECT $1, entity_key, $3 FROM ins) \
         INSERT INTO entity_search (context_space_key, entity_key, type_ids, attribute_name_ids) SELECT $1, entity_key, ARRAY[]::integer[], ARRAY[]::integer[] FROM ins",
    )
    .bind(context_space_key)
    .bind(&id)
    .bind(&payload)
    .execute(scope.database_pool())
    .await
    .expect_err("a typeless Entity row must be refused");
    let database_error = failure.as_database_error().expect("the refusal must come from the database");
    assert_eq!(
        database_error.code().as_deref(),
        Some("23514"),
        "the refusal must be the non-empty types check: {database_error}"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_persists_every_type_of_a_multi_type_entity() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:multi-type");

    let entity = json!({
        "id": id,
        "type": ["Building", "TouristDestination"],
        "temperature": {"type": "Property", "value": 1.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let types: Vec<String> = sqlx::query_scalar(
        "SELECT n.name FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key \
         CROSS JOIN unnest(s.type_ids) AS t(id) JOIN entity_type_names n ON n.id = t.id WHERE e.id = $1 ORDER BY 1",
    )
    .bind(&id)
    .fetch_all(scope.database_pool())
    .await
    .expect("types must be readable");
    assert!(
        types.contains(&BUILDING_TYPE.to_owned()),
        "the expanded Building type must be stored: {types:?}"
    );
    assert_eq!(types.len(), 2, "both declared types must be stored: {types:?}");

    scope.finish().await.expect("cleanup failed");
}
