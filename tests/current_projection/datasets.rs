//! Default dataset uniqueness and distinct datasets: the default instance is
//! unique per attribute, and each `datasetId` forms its own grain.
//!
//! Clause 4.5.5 makes the instance without a `datasetId` the default instance,
//! and clause 5.2.5 makes it unique per attribute. The projection states that
//! with `UNIQUE NULLS NOT DISTINCT (entity_key, attribute_name_key,
//! dataset_id)`, so a second default write updates the one grain instead of
//! adding a second.

use crate::projection_rows::{attribute_name_key, entity_key, rows_for_attribute};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// An absolute-IRI `datasetId`.
const HTTP_DATASET: &str = "http://example.test/d1";

/// A URN `datasetId`.
const URN_DATASET: &str = "urn:ngsi-ld:Dataset:d2";

#[tokio::test]
async fn current_projection_second_default_write_shares_one_grain() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:default-grain");

    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 1.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let second = json!({
        "temperature": {"type": "Property", "value": 2.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let appended = client.append_entity_attrs(&id, &second).await.expect("append attributes failed");
    assert!(
        appended.status == StatusCode::NO_CONTENT || appended.status == StatusCode::CREATED,
        "second default write: {:?} {:?}",
        appended.status,
        appended.body
    );

    let rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    assert_eq!(rows.len(), 1, "the default instance is one grain, not two: {rows:?}");
    assert_eq!(rows[0].dataset_id, None);
    assert_eq!(rows[0].value_text.as_deref(), Some("2"), "the surviving grain holds the later write");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_refuses_a_second_default_row_for_one_attribute() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:default-uniqueness");

    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 1.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let key = entity_key(scope.database_pool(), &id)
        .await
        .expect("the created Entity must have a surrogate key");
    let name_key = attribute_name_key(scope.database_pool(), &id, "temperature").await;

    // A writer bypassing the broker still cannot create a second default
    // instance: `NULLS NOT DISTINCT` makes two absent dataset identifiers the
    // same grain, which a plain unique constraint would not.
    let failure = sqlx::query(
        "INSERT INTO current_attributes \
         (entity_key, attribute_name_key, ordinal, attribute_kind, value_kind, value_number, created_at, modified_at) \
         VALUES ($1, $2, 1, 1, 1, 9, now(), now())",
    )
    .bind(key)
    .bind(name_key)
    .execute(scope.database_pool())
    .await
    .expect_err("a second default instance must conflict");
    let database_error = failure.as_database_error().expect("the refusal must come from the database");
    assert_eq!(
        database_error.code().as_deref(),
        Some("23505"),
        "the refusal must be a unique violation: {database_error}"
    );

    assert_eq!(rows_for_attribute(scope.database_pool(), &id, "temperature").await.len(), 1);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_three_distinct_datasets_form_three_grains() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:distinct-datasets");

    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": [
            {"type": "Property", "value": 1.0},
            {"type": "Property", "value": 2.0, "datasetId": HTTP_DATASET},
            {"type": "Property", "value": 3.0, "datasetId": URN_DATASET}
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    let grains: Vec<(Option<&str>, Option<&str>)> = rows.iter().map(|row| (row.dataset_id.as_deref(), row.value_text.as_deref())).collect();
    assert_eq!(
        grains,
        vec![(None, Some("1")), (Some(HTTP_DATASET), Some("2")), (Some(URN_DATASET), Some("3"))],
        "an absent, an HTTP and a URN dataset identifier are three grains"
    );

    let all = client
        .get_entity(&id, "attrs=temperature", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(all.status, StatusCode::OK);
    let all_body = all.json_value().expect("retrieval body must be JSON");
    assert_eq!(
        all_body["temperature"].as_array().map(Vec::len),
        Some(3),
        "retrieval without a dataset filter returns every instance: {all_body:?}"
    );

    let default_only = client
        .get_entity(&id, "attrs=temperature&datasetId=%40none", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve default instance failed");
    assert_eq!(default_only.status, StatusCode::OK);
    let default_body = default_only.json_value().expect("retrieval body must be JSON");
    assert_eq!(
        default_body["temperature"],
        json!({"type": "Property", "value": 1}),
        "`@none` selects the default instance alone: {default_body:?}"
    );
    assert!(
        default_body["temperature"].get("datasetId").is_none(),
        "the default instance carries no datasetId: {default_body:?}"
    );

    scope.finish().await.expect("cleanup failed");
}
