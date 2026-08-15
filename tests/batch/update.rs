#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use reqwest::{Method, StatusCode};
use serial_test::serial;

use crate::helpers::*;
use serde_json::json;

#[tokio::test]
#[serial]
async fn batch_update_properties_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:005_01_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:005_01_01b");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        create_entity(&c, &entity).await;
    }

    let u1 = building_relationship(&id1);
    let u2 = building_relationship(&id2);
    let updates = json!([u1, u2]);

    let (status, _) = batch_update(&c, &updates, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "batch update should return 204");

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_update_relationships_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:005_01_02a");
    let id2 = unique_id("urn:ngsi-ld:Building:005_01_02b");

    for id in [&id1, &id2] {
        let entity = building_minimal(id);
        create_entity(&c, &entity).await;
    }

    let u1 = building_relationship(&id1);
    let u2 = building_relationship(&id2);
    let updates = json!([u1, u2]);

    let (status, _) = batch_update(&c, &updates, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
}

#[tokio::test]
#[serial]
async fn batch_update_relationship_of_property_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:005_01_03a");
    let id2 = unique_id("urn:ngsi-ld:Building:005_01_03b");

    for id in [&id1, &id2] {
        let entity = building_minimal(id);
        create_entity(&c, &entity).await;
    }

    let u1 = building_relationship_of_property(&id1);
    let u2 = building_relationship_of_property(&id2);
    let updates = json!([u1, u2]);

    let (status, _) = batch_update(&c, &updates, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
#[serial]
async fn batch_update_no_overwrite_existing_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:005_02_01");
    let entity = building_simple_attributes(&id);
    create_entity(&c, &entity).await;

    let u1 = building_simple_attributes(&id);
    let updates = json!([u1]);

    let (status, _) = batch_update(&c, &updates, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "existing attr with noOverwrite should return 207");

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_update_no_overwrite_new_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:005_02_02");
    let entity = building_minimal(&id);
    create_entity(&c, &entity).await;

    let u1 = building_relationship(&id);
    let updates = json!([u1]);

    let (status, _) = batch_update(&c, &updates, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "new attr with noOverwrite should return 204");

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
}

#[tokio::test]
#[serial]
async fn batch_update_no_overwrite_mixed_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:005_02_03");
    let entity = building_simple_attributes(&id);
    create_entity(&c, &entity).await;

    let u1 = building_relationship_of_property(&id);
    let updates = json!([u1]);

    let (status, _) = batch_update(&c, &updates, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);
}

#[tokio::test]
#[serial]
async fn batch_update_mixed_existing_nonexisting_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let existing_id1 = unique_id("urn:ngsi-ld:Building:005_03_01a");
    let existing_id2 = unique_id("urn:ngsi-ld:Building:005_03_01b");
    let nonexistent_id = unique_id("urn:ngsi-ld:Building:005_03_01c");

    for id in [&existing_id1, &existing_id2] {
        let entity = building_simple_attributes(id);
        create_entity(&c, &entity).await;
    }

    let u1 = building_simple_attributes(&existing_id1);
    let u2 = building_simple_attributes(&existing_id2);
    let u3 = building_simple_attributes(&nonexistent_id);
    let updates = json!([u1, u2, u3]);

    let (status, body) = batch_update(&c, &updates, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    let success = body["success"].as_array().expect("should have success");
    let errors = body["errors"].as_array().expect("should have errors");
    pretty_assertions::assert_eq!(success.len(), 2);
    pretty_assertions::assert_eq!(errors.len(), 1);
}

#[tokio::test]
async fn batch_update_invalid_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(
        &c,
        Method::POST,
        "/entityOperations/update",
        b"[{\"id\":\"urn:ngsi-ld:Building:x\",,}]",
        Some("application/ld+json"),
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_update_invalid_json_ld_returns_207() {
    let c = client();
    let updates = json!([{
        "id": "urn:ngsi-ld:Building:invalid_ld",
        "invalidField": "not a valid NGSI-LD structure",
        "@context": COMPOUND_CONTEXT
    }]);

    let (status, _) = batch_update(&c, &updates, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "invalid JSON-LD should return 207");
}

#[tokio::test]
#[serial]
async fn batch_update_entity_type_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:005_05_01");
    let entity = building_minimal(&id);
    create_entity(&c, &entity).await;

    let u1 = building_minimal_with_new_types(&id);
    let updates = json!([u1]);

    let (status, _) = batch_update(&c, &updates, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
}
