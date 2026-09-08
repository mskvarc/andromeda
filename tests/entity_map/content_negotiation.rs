//! Content negotiation on the `EntityMap` resource (NGSI-LD 6.3.5).

use crate::{clean_database, cleanup_entity_map, client, create_entity_map_from_get, delete_entity, get_entity_map, setup_single_vehicle, unique_id};
use reqwest::StatusCode;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn entity_map_content_negotiation_accept_application_json_returns_link_header_without_context_body() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-cn-json");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, headers, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(headers.get("Link").is_some());
    assert!(body.get("@context").is_none());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_content_negotiation_accept_application_ld_json_returns_context_in_body() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-cn-ldjson");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, _, body) = get_entity_map(&c, &map_id, "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_some());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}
