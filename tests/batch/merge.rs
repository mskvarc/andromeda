#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use crate::helpers::*;
use andromeda_test_fixtures::context::{COMPOUND_CONTEXT, CORE_CONTEXT};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::{Method, StatusCode};
use serde_json::json;

#[tokio::test]
async fn batch_merge_empty_fragment_preserves_entities_returns_204() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:057_01_01a");
    let id2 = scope.id("Building:057_01_01b");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        let (status, _, _) = create_entity(&c, &entity).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let e1 = building_minimal(&id1);
    let e2 = building_minimal(&id2);
    let fragments = json!([e1, e2]);

    let (status, _) = batch_merge(&c, &fragments).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "batch merge should return 204");

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_merge_properties_updates_values_returns_204() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:057_01_02a");
    let id2 = scope.id("Building:057_01_02b");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        create_entity(&c, &entity).await;
    }

    let e1 = building_relationship(&id1);
    let e2 = building_relationship(&id2);
    let fragments = json!([e1, e2]);

    let (status, _) = batch_merge(&c, &fragments).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    pretty_assertions::assert_eq!(body["subCategory"]["value"], "tourism");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_merge_relationship_adds_to_entities_returns_204() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:057_01_03a");
    let id2 = scope.id("Building:057_01_03b");

    for id in [&id1, &id2] {
        let entity = building_minimal(id);
        create_entity(&c, &entity).await;
    }

    let e1 = building_relationship(&id1);
    let e2 = building_relationship(&id2);
    let fragments = json!([e1, e2]);

    let (status, _) = batch_merge(&c, &fragments).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_merge_partial_preserves_sub_attributes_returns_204() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:057_01_04a");
    let id2 = scope.id("Building:057_01_04b");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        create_entity(&c, &entity).await;
    }

    let e1 = building_relationship_of_property(&id1);
    let e2 = building_relationship_of_property(&id2);
    let fragments = json!([e1, e2]);

    let (status, _) = batch_merge(&c, &fragments).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["airQualityLevel"]["value"], 4);
    pretty_assertions::assert_eq!(body["airQualityLevel"]["unitCode"], "C62", "unitCode should be preserved");
    pretty_assertions::assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_merge_mixed_existing_nonexisting_returns_207() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let existing_id1 = scope.id("Building:057_02_01a");
    let existing_id2 = scope.id("Building:057_02_01b");
    let nonexistent_id = scope.id("Building:057_02_01c");

    for id in [&existing_id1, &existing_id2] {
        let entity = building_simple_attributes(id);
        create_entity(&c, &entity).await;
    }

    let e1 = building_simple_attributes(&existing_id1);
    let e2 = building_simple_attributes(&existing_id2);
    let e3 = building_simple_attributes(&nonexistent_id);
    let fragments = json!([e1, e2, e3]);

    let (status, body) = batch_merge(&c, &fragments).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    let success = body["success"].as_array().expect("should have success");
    let errors = body["errors"].as_array().expect("should have errors");
    pretty_assertions::assert_eq!(success.len(), 2);
    pretty_assertions::assert_eq!(errors.len(), 1);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_merge_invalid_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(
        &c,
        Method::POST,
        "/entityOperations/merge",
        b"[{\"id\":\"urn:ngsi-ld:Building:x\",,}]",
        Some("application/ld+json"),
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_merge_invalid_json_ld_returns_207() {
    let c = client();
    let fragments = json!([{
        "id": "urn:ngsi-ld:Building:invalid_ld",
        "invalidField": "not a valid NGSI-LD structure",
        "@context": COMPOUND_CONTEXT
    }]);

    let (status, _) = batch_merge(&c, &fragments).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "invalid JSON-LD should return 207");
}

/// NGSI-LD 5.5.11.5: when an array repeats an Entity ID, "these merge
/// operations shall be done in chronological order" and "only the entity
/// resulting from merging all of the entity instances, in the correct order, is
/// maintained in the current state". Both fragments therefore end up in the
/// stored entity, the later one composed onto the earlier one's result.
#[tokio::test]
async fn batch_merge_repeated_id_composes_the_instances_in_order_returns_204() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let id = scope.id("Building:057_06_01");
    let entity = building_simple_attributes(&id);
    let (status, _, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let first = json!({
        "id": &id,
        "type": "Building",
        "name": {"type": "Property", "value": "Tour Eiffel"},
        "@context": [CORE_CONTEXT]
    });
    let second = json!({
        "id": &id,
        "type": "Building",
        "subCategory": {"type": "Property", "value": "landmark"},
        "@context": [CORE_CONTEXT]
    });
    let fragments = json!([first, second]);

    let (status, _) = batch_merge(&c, &fragments).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "Tour Eiffel", "the first fragment's merge must survive the second");
    pretty_assertions::assert_eq!(
        body["subCategory"]["value"],
        "landmark",
        "the second fragment must merge onto the first fragment's result"
    );
    scope.finish().await.expect("cleanup failed");
}
