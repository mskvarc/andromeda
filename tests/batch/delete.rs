use reqwest::{Method, StatusCode};
use serial_test::serial;

use crate::helpers::*;
use serde_json::json;

#[tokio::test]
#[serial]
async fn batch_delete_entities_returns_204() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:006_01_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:006_01_01b");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        let (status, _, _) = create_entity(&c, &entity).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let ids = json!([&id1, &id2]);
    let (status, _) = batch_delete(&c, &ids).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "batch delete should return 204");

    let (status, _, _) = get_entity(&c, &id1, "", false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = get_entity(&c, &id2, "", false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn batch_delete_mixed_existing_nonexisting_returns_207() {
    clean_database().await;
    let c = client();

    let existing_id = unique_id("urn:ngsi-ld:Building:006_02_01a");
    let nonexistent_id = unique_id("urn:ngsi-ld:Building:006_02_01b");

    let entity = building_simple_attributes(&existing_id);
    let (status, _, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let ids = json!([&existing_id, &nonexistent_id]);
    let (status, body) = batch_delete(&c, &ids).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "mixed should return 207");

    let success = body["success"].as_array().expect("should have success array");
    let errors = body["errors"].as_array().expect("should have errors array");
    assert_eq!(success.len(), 1);
    assert_eq!(errors.len(), 1);

    let (status, _, _) = get_entity(&c, &existing_id, "", false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn batch_delete_invalid_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(
        &c,
        Method::POST,
        "/entityOperations/delete",
        b"[\"urn:ngsi-ld:Building:x\",,]",
        Some("application/json"),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_delete_empty_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/delete", b"[]", Some("application/json"), false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn batch_delete_duplicate_ids_returns_207() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:006_04_01");
    let entity = building_simple_attributes(&id);
    let (status, _, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let ids = json!([&id, &id]);
    let (status, body) = batch_delete(&c, &ids).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "duplicate IDs should return 207");

    let success = body["success"].as_array().expect("should have success array");
    let errors = body["errors"].as_array().expect("should have errors array");
    assert_eq!(success.len(), 1, "first delete should succeed");
    assert_eq!(errors.len(), 1, "second delete should fail");
}
