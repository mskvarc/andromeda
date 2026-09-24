//! `EntityMaps` created from a temporal query (NGSI-LD 5.2.32, 6.3.19).

use crate::{
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
    vehicle_linking_to,
};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::json;

/// Clause 5.14.5.4: "If a temporal query is not provided then an error of type
/// `BadRequestData` shall be raised", so the GET form carries the clause-4.11
/// `timerel`/`timeAt` pair alongside the entity type selector clause 5.14.5.4
/// also demands.
#[tokio::test]
async fn entity_map_temporal_create_from_get_returns_201() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-temp");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_temporal_entity_map_from_get(&c, &scope, "type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.14.5.4: "If a temporal query is not provided then an error of type
/// `BadRequestData` shall be raised", so the POST form carries the clause-5.2.21
/// `temporalQ` member of the clause-5.2.23 Query.
#[tokio::test]
async fn entity_map_temporal_create_from_post_returns_201() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-temp-post");
    setup_single_vehicle(&c, &id).await;

    let body = json!({
        "type": "Query",
        "entities": [{ "type": "Vehicle" }],
        "temporalQ": {
            "timerel": "before",
            "timeAt": "2030-01-01T00:00:00Z"
        }
    });
    let (status, _, body) = create_temporal_entity_map_from_post(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14, over the Temporal API: the map a joined temporal query
/// creates records the Linked Entities the traversal reached as well as the
/// roots it matched.
#[tokio::test]
async fn temporal_entity_map_records_linked_ids() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let linked = scope.id("Vehicle:em-temp-join-B");
    let root = scope.id("Vehicle:em-temp-join-A");

    setup_single_vehicle(&c, &linked).await;
    let (status, _) = create_entity(&c, &vehicle_linking_to(&root, &linked)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("id={root}&type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z&entityMap=true&join=flat&joinLevel=1");
    let (status, headers, body) = query_temporal_entities(&c, &scope, &query).await;
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
    scope.finish().await.expect("cleanup failed");
}
