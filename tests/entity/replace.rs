//! Entity replacement tests.
//!
//! Tests entity replacement (PUT) operations:
//! - Replace existing entity (createdAt preserved)
//! - Invalid ID
//! - Missing ID (PUT on collection)
//! - Non-existent entity
//! - Replace with scopes

use crate::{
    COMPOUND_CONTEXT,
    TEST_SUITE_CONTEXT,
    building_locatedat_and_name,
    building_simple_attributes,
    building_with_many_scopes,
    building_with_one_scope,
    client,
    create_entity_json,
    get_entity,
    replace_entity,
    request_method_on_collection,
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Number, json};
use std::str::FromStr;

#[tokio::test]
async fn entity_replace_success_createdat_preserved() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:replace_054_01_01");

    let entity = building_simple_attributes(&id);
    let status = create_entity_json(&c, &entity, COMPOUND_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, before) = get_entity(&c, &id, "options=sysAttrs", "application/json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let created_at = before["createdAt"].as_str().unwrap().to_string();

    let replacement = building_locatedat_and_name(&id);
    let (status, _) = replace_entity(&c, &id, &replacement, "application/json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "replace should return 204");

    let (status, after) = get_entity(&c, &id, "options=sysAttrs", "application/json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after["name"]["value"], "Pisa Tower", "name should be updated");
    assert_eq!(after["locatedAt"]["object"], "urn:ngsi-ld:City:Pisa", "new relationship should be present");
    assert!(after.get("subCategory").is_none(), "old attribute should be gone");
    assert_eq!(after["createdAt"].as_str().unwrap(), created_at, "createdAt should be preserved");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_replace_preserves_an_exact_decimal() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:replace_exact_number");
    assert_eq!(
        create_entity_json(&c, &building_simple_attributes(&id), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );
    let exact = Number::from_str("0.120000000000000005").expect("exact JSON number");
    let replacement = json!({"id": id, "type": "Building", "reading": {"type": "Property", "value": exact}, "@context": TEST_SUITE_CONTEXT});

    let (status, _) = replace_entity(&c, &id, &replacement, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(serde_json::to_string(&body).expect("entity JSON").contains("0.120000000000000005"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_replace_invalid_id_returns_400() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let replacement = json!({
        "id": "invalidUri",
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity(&c, "invalidUri", &replacement, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid ID should return 400");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_replace_missing_id_returns_error() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let replacement = json!({
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_method_on_collection(&c, Method::PUT, Some(&replacement)).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::METHOD_NOT_ALLOWED,
        "missing ID should return 400 or 405, got {status}",
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_replace_not_found_returns_404() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:replace_054_02_03");

    let replacement = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity(&c, &id, &replacement, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_replace_with_scopes_success() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:replace_054_03_01");

    let entity = building_with_one_scope(&id);
    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let replacement = building_with_many_scopes(&id);
    let (status, _) = replace_entity(&c, &id, &replacement, "application/json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "replace should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    let scopes = body["scope"].as_array().expect("scope should be an array");
    assert_eq!(scopes.len(), 2);
    scope.finish().await.expect("cleanup failed");
}
