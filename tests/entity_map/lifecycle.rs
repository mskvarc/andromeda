//! Lifecycle of an `EntityMap` resource: retrieving, updating and deleting one
//! that a query already created (NGSI-LD 5.2.32, 6.3.19).

use crate::{
    BAD_REQUEST_DATA_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    cleanup_entity_map,
    client,
    create_entity_map_from_get,
    delete_entity,
    delete_entity_map,
    get_entity_map,
    setup_single_vehicle,
    update_entity_map,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, Duration, SubsecRound, Utc};
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn entity_map_retrieve_existing_map_returns_200() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-retr");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_retrieve_nonexistent_map_returns_404() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let (status, _, body) = get_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent", "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_retrieve_invalid_map_id_returns_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let (status, _, body) = get_entity_map(&c, "not-a-valid-uri", "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_update_valid_expires_at_returns_204() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-upd");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let new_expires = (Utc::now() + Duration::hours(2)).to_rfc3339();
    let patch = json!({ "expiresAt": new_expires });
    let (status, _) = update_entity_map(&c, &map_id, "application/json", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_update_output_only_fields_ignored_returns_204() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-upd-oof");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({
        "expiresAt": (Utc::now() + Duration::hours(3)).to_rfc3339(),
        "entityMap": { "urn:fake": ["@none"] },
        "linkedMaps": { "fake": "fake" }
    });
    let (status, _) = update_entity_map(&c, &map_id, "application/json", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.14.2.3 takes "A JSON-LD document": an `application/ld+json` body
/// names its context in a top-level `@context`, which is no member of the
/// update.
#[tokio::test]
async fn entity_map_update_ld_json_with_context_returns_204() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-upd-ld");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let new_expires = (Utc::now() + Duration::hours(4)).trunc_subsecs(0);
    let patch = json!({ "@context": [CORE_CONTEXT], "expiresAt": new_expires.to_rfc3339() });
    let (status, body) = update_entity_map(&c, &map_id, "application/ld+json", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let stored = body["expiresAt"]
        .as_str()
        .expect("expiresAt is a string")
        .parse::<DateTime<Utc>>()
        .expect("expiresAt is a timestamp");
    pretty_assertions::assert_eq!(stored, new_expires);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.5: an `application/ld+json` body without `@context`, and an
/// `application/json` body carrying one, are `BadRequestData`.
#[tokio::test]
async fn entity_map_update_context_in_the_wrong_body_or_none_in_the_right_one_returns_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-upd-ctx");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let expires = (Utc::now() + Duration::hours(4)).to_rfc3339();
    for (content_type, patch) in [
        ("application/ld+json", json!({ "expiresAt": expires })),
        ("application/json", json!({ "@context": [CORE_CONTEXT], "expiresAt": expires })),
    ] {
        let (status, body) = update_entity_map(&c, &map_id, content_type, &patch).await;
        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "{content_type}");
        pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{content_type}");
    }

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_update_nonexistent_map_returns_404() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let patch = json!({ "expiresAt": (Utc::now() + Duration::hours(1)).to_rfc3339() });
    let (status, body) = update_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent", "application/json", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_update_invalid_expires_at_returns_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-upd-bad");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({ "expiresAt": "not-a-date" });
    let (status, body) = update_entity_map(&c, &map_id, "application/json", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_update_past_expires_at_returns_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-upd-past");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({ "expiresAt": (Utc::now() - Duration::minutes(1)).to_rfc3339() });
    let (status, body) = update_entity_map(&c, &map_id, "application/json", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_delete_existing_map_then_retrieve_returns_404() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-del");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let status = delete_entity_map(&c, &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());

    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_delete_nonexistent_map_returns_404() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let status = delete_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}
