//! Content negotiation on the `EntityMap` resource (NGSI-LD 6.3.5).

use crate::{cleanup_entity_map, client, create_entity_map_from_get, delete_entity, get_entity_map, setup_single_vehicle};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;

#[tokio::test]
async fn entity_map_content_negotiation_accept_application_json_returns_link_header_without_context_body() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-cn-json");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, headers, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(headers.get("Link").is_some());
    assert!(body.get("@context").is_none());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_content_negotiation_accept_application_ld_json_returns_context_in_body() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-cn-ldjson");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, _, body) = get_entity_map(&c, &map_id, "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_some());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}
