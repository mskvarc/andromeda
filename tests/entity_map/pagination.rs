//! Paging a query against a frozen `EntityMap` (NGSI-LD 5.2.32, 6.3.19).
//!
//! The map fixes the result set at creation time, so later writes cannot make a
//! page overlap another or skip an entity.

use crate::{BASE_URL, cleanup_entity_map, client, create_entity_map_from_get, delete_entity, query_entities_with_entity_map, setup_single_vehicle};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use std::collections::HashSet;
use urlencoding::encode;

#[tokio::test]
async fn entity_map_pagination_query_with_frozen_map_covers_all_ids_without_duplicates() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let mut created_ids = Vec::new();

    for index in 0..10 {
        let id = scope.id(&format!("Vehicle:em-pg-{index}"));
        setup_single_vehicle(&c, &id).await;
        created_ids.push(id);
    }

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 10);

    let page_queries = [
        "type=Vehicle&limit=3&offset=0",
        "type=Vehicle&limit=3&offset=3",
        "type=Vehicle&limit=3&offset=6",
        "type=Vehicle&limit=3&offset=9",
    ];

    let mut collected_ids = Vec::new();

    for (page_index, query) in page_queries.iter().enumerate() {
        let (status, headers, body) = query_entities_with_entity_map(&c, &scope, query, &map_id).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK);

        assert!(headers.get("NGSILD-EntityMap").is_some());

        let arr = body.as_array().expect("query result should be array");
        if page_index < 3 {
            pretty_assertions::assert_eq!(arr.len(), 3);
        } else {
            pretty_assertions::assert_eq!(arr.len(), 1);
        }

        for item in arr {
            let entity_id = item["id"].as_str().expect("entity should have id").to_owned();
            collected_ids.push(entity_id);
        }
    }

    pretty_assertions::assert_eq!(collected_ids.len(), 10);
    let dedup: HashSet<&String> = collected_ids.iter().collect();
    pretty_assertions::assert_eq!(dedup.len(), 10);

    cleanup_entity_map(&c, &map_id).await;
    for id in created_ids {
        delete_entity(&c, &id).await;
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_pagination_new_entity_created_after_map_is_not_in_results() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Vehicle:em-new1");
    setup_single_vehicle(&c, &id1).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let id2 = scope.id("Vehicle:em-new2");
    setup_single_vehicle(&c, &id2).await;

    let (status, _, body) = query_entities_with_entity_map(&c, &scope, "type=Vehicle&limit=100", &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result should be array");
    let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();

    assert!(ids.contains(&id1.as_str()));
    assert!(!ids.contains(&id2.as_str()));

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_pagination_deleted_entity_is_skipped_from_results() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Vehicle:em-dsk1");
    let id2 = scope.id("Vehicle:em-dsk2");
    setup_single_vehicle(&c, &id1).await;
    setup_single_vehicle(&c, &id2).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 2);

    let status = client()
        .delete(format!("{BASE_URL}/entities/{}", encode(&id1)))
        .send()
        .await
        .expect("delete_entity request failed")
        .status();
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = query_entities_with_entity_map(&c, &scope, "type=Vehicle&limit=100", &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result should be array");
    pretty_assertions::assert_eq!(arr.len(), 1);
    pretty_assertions::assert_eq!(arr[0]["id"], id2);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id2).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_pagination_query_with_count_returns_results_count_header() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let mut ids = Vec::new();

    for index in 0..5 {
        let id = scope.id(&format!("Vehicle:em-cnt-{index}"));
        setup_single_vehicle(&c, &id).await;
        ids.push(id);
    }

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, headers, _) = query_entities_with_entity_map(&c, &scope, "type=Vehicle&count=true&limit=2", &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count_header = headers.get("NGSILD-Results-Count").expect("count header should be present");
    let count = count_header
        .to_str()
        .expect("header should be ASCII")
        .parse::<usize>()
        .expect("count should be numeric");
    pretty_assertions::assert_eq!(count, 5);

    cleanup_entity_map(&c, &map_id).await;
    for id in ids {
        delete_entity(&c, &id).await;
    }
    scope.finish().await.expect("cleanup failed");
}
