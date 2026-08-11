//! EntityMaps created from a temporal query (NGSI-LD 5.2.32, 6.3.19).

use crate::{
    clean_database,
    cleanup_entity_map,
    client,
    create_entity,
    create_temporal_entity_map_from_get,
    create_temporal_entity_map_from_post,
    delete_entity,
    entity_map_header,
    get_entity_map,
    mapped_ids,
    query_temporal_entities,
    setup_single_vehicle,
    unique_id,
    vehicle_linking_to,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn entity_map_temporal_create_from_get_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-temp");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_temporal_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_temporal_create_from_post_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-temp-post");
    setup_single_vehicle(&c, &id).await;

    let body = json!({
        "type": "Query",
        "entities": [{ "type": "Vehicle" }]
    });
    let (status, _, body) = create_temporal_entity_map_from_post(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

/// Clause 5.5.14, over the Temporal API: the map a joined temporal query
/// creates records the Linked Entities the traversal reached as well as the
/// roots it matched.
#[tokio::test]
#[serial]
async fn temporal_entity_map_records_linked_ids() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let linked = unique_id("urn:ngsi-ld:Vehicle:em-temp-join-B");
    let root = unique_id("urn:ngsi-ld:Vehicle:em-temp-join-A");

    setup_single_vehicle(&c, &linked).await;
    let (status, _) = create_entity(&c, &vehicle_linking_to(&root, &linked)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("id={root}&type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z&entityMap=true&join=flat&joinLevel=1");
    let (status, headers, body) = query_temporal_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");

    let map_id = entity_map_header(&headers);

    let (status, _, map) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = mapped_ids(&map);
    assert!(ids.contains(&root), "the matched root must be mapped: {ids:?}");
    assert!(ids.contains(&linked), "the joined Linked Entity must be mapped: {ids:?}");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &root).await;
    delete_entity(&c, &linked).await;
}
