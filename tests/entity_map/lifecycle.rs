//! Lifecycle of an EntityMap resource: retrieving, updating and deleting one
//! that a query already created (NGSI-LD 5.2.32, 6.3.19).

use crate::{
    BAD_REQUEST_DATA_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    clean_database,
    cleanup_entity_map,
    client,
    create_entity_map_from_get,
    delete_entity,
    delete_entity_map,
    get_entity_map,
    setup_single_vehicle,
    unique_id,
    update_entity_map,
};
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn entity_map_retrieve_existing_map_returns_200() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-retr");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map should have id").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");
    assert!(body["expiresAt"].is_string());
    assert!(body["entityMap"].is_object());
    assert!(body["linkedMaps"].is_object());

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    for brokers in entity_map.values() {
        let arr = brokers.as_array().expect("entity map value should be array");
        pretty_assertions::assert_eq!(arr.len(), 1);
        pretty_assertions::assert_eq!(arr[0], "@none");
    }
    assert!(body["linkedMaps"].as_object().expect("linkedMaps should be object").is_empty());

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["id"], map_id);
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_retrieve_nonexistent_map_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, _, body) = get_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent", "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
}

#[tokio::test]
#[serial]
async fn entity_map_retrieve_invalid_map_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, _, body) = get_entity_map(&c, "not-a-valid-uri", "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[tokio::test]
#[serial]
async fn entity_map_update_valid_expires_at_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let new_expires = (Utc::now() + Duration::hours(2)).to_rfc3339();
    let patch = json!({ "expiresAt": new_expires });
    let (status, _) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_update_output_only_fields_ignored_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd-oof");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({
        "expiresAt": (Utc::now() + Duration::hours(3)).to_rfc3339(),
        "entityMap": { "urn:fake": ["@none"] },
        "linkedMaps": { "fake": "fake" }
    });
    let (status, _) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_update_nonexistent_map_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let patch = json!({ "expiresAt": (Utc::now() + Duration::hours(1)).to_rfc3339() });
    let (status, body) = update_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
}

#[tokio::test]
#[serial]
async fn entity_map_update_invalid_expires_at_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd-bad");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({ "expiresAt": "not-a-date" });
    let (status, body) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_update_past_expires_at_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd-past");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({ "expiresAt": (Utc::now() - Duration::minutes(1)).to_rfc3339() });
    let (status, body) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_delete_existing_map_then_retrieve_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-del");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let status = delete_entity_map(&c, &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_delete_nonexistent_map_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let status = delete_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}
