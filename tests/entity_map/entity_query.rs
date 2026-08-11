//! Querying entities with an EntityMap alongside (NGSI-LD 5.5.14, 6.3.19).

use crate::{
    clean_database,
    cleanup_entity_map,
    client,
    create_entity,
    delete_entity,
    entity_map_header,
    get_entity_map,
    mapped_ids,
    query_entities,
    setup_single_vehicle,
    unique_id,
    vehicle_linking_to,
};
use reqwest::StatusCode;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn entity_map_query_entities_with_entity_map_true_returns_201_and_header() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-int");
    setup_single_vehicle(&c, &id).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle&entityMap=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = entity_map_header(&headers);

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    assert!(entity_map.contains_key(&id));

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_query_entities_without_entity_map_returns_200_without_header() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-int-false");
    setup_single_vehicle(&c, &id).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(headers.get("NGSILD-EntityMap").is_none());

    delete_entity(&c, &id).await;
}

/// Clause 5.5.14: an EntityMap created alongside a joined query records the
/// entities the traversal reached as well as the ones the query matched, so a
/// later page against the frozen map still resolves the same graph.
#[tokio::test]
#[serial]
async fn entity_map_records_linked_ids() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let linked = unique_id("urn:ngsi-ld:Vehicle:em-join-B");
    let root = unique_id("urn:ngsi-ld:Vehicle:em-join-A");

    setup_single_vehicle(&c, &linked).await;
    let (status, _) = create_entity(&c, &vehicle_linking_to(&root, &linked)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, body) = query_entities(&c, &format!("id={root}&type=Vehicle&entityMap=true&join=flat&joinLevel=1")).await;
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
