//! Batch entity deletion tests.
//!
//! Tests entity deletion by query operations:
//! - Delete by type
//! - Delete by ID list
//! - Delete by ID pattern
//! - Delete by q filter
//! - Empty result returns 204
//! - No filters returns 400

use crate::{TEST_SUITE_CONTEXT, building_simple_attributes, client, create_entity_json, delete_entities_by_query, get_entity, vehicle_simple_attributes};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn entity_delete_by_query_by_type_success() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:dq_type_1");
    let id2 = scope.id("Building:dq_type_2");
    let id3 = scope.id("Building:dq_type_3");
    let id_vehicle = scope.id("Vehicle:dq_type_v");

    for id in [&id1, &id2, &id3] {
        let entity = building_simple_attributes(id);
        let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let vehicle = vehicle_simple_attributes(&id_vehicle);
    let status = create_entity_json(&c, &vehicle, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entities_by_query(&c, &scope, "type=Building&local=true", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by type should return 204");

    for id in [&id1, &id2, &id3] {
        let (status, _) = get_entity(&c, id, "", "application/json", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "building {id} should be deleted");
    }

    let (status, _) = get_entity(&c, &id_vehicle, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK, "vehicle should still exist");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_delete_by_query_by_id_success() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:dq_id_1");
    let id2 = scope.id("Building:dq_id_2");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let ids = format!("{id1},{id2}");
    let query = format!("id={}&local=true", urlencoding::encode(&ids));
    let (status, _) = delete_entities_by_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by id should return 204");

    for id in [&id1, &id2] {
        let (status, _) = get_entity(&c, id, "", "application/json", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "entity {id} should be deleted");
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_delete_by_query_by_id_pattern_success() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:dq_pat_match1");
    let id2 = scope.id("Building:dq_pat_match2");
    let id_other = scope.id("Building:dq_pat_other");

    for id in [&id1, &id2, &id_other] {
        let entity = building_simple_attributes(id);
        let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let pattern = urlencoding::encode(&format!("^{}:Building:dq_pat_match.*", scope.id_prefix())).into_owned();
    let query = format!("idPattern={pattern}&local=true");
    let (status, _) = delete_entities_by_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by idPattern should return 204");

    for id in [&id1, &id2] {
        let (status, _) = get_entity(&c, id, "", "application/json", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "matched entity {id} should be deleted");
    }

    let (status, _) = get_entity(&c, &id_other, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK, "unmatched entity should remain");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_delete_by_query_by_q_filter_success() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let id_high = scope.id("Building:dq_q_high");
    let id_low = scope.id("Building:dq_q_low");

    let mut entity_high = building_simple_attributes(&id_high);
    entity_high["airQualityLevel"]["value"] = json!(50);
    let status = create_entity_json(&c, &entity_high, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let mut entity_low = building_simple_attributes(&id_low);
    entity_low["airQualityLevel"]["value"] = json!(2);
    let status = create_entity_json(&c, &entity_low, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let q = urlencoding::encode("airQualityLevel>30");
    let query = format!("type=Building&q={q}&local=true");
    let (status, _) = delete_entities_by_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by q filter should return 204");

    let (status, _) = get_entity(&c, &id_high, "", "application/json", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "high value entity should be deleted");

    let (status, _) = get_entity(&c, &id_low, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK, "low value entity should remain");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.21.4 defines no `join` for delete by query: the operation renders
/// nothing and retrieves nothing, so a `q` hop into a Linked Entity and a nested
/// `pick` term both name something the operation can never reach, whatever else
/// the request carries.
#[tokio::test]
async fn entity_delete_by_query_rejects_linked_entity_references() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let traversing_q = urlencoding::encode("isParkedIn{category}==\"commercial\"");
    let nested_pick = urlencoding::encode("isParkedIn{category}");
    // Both requests carry `join=flat`, which is exactly what would make them
    // legal on a query. The rejection is the operation's own, not the clause
    // 5.7.2.4 consistency rule firing for a missing join.
    let queries = [
        format!("type=Building&q={traversing_q}&join=flat&local=true"),
        format!("type=Building&pick={nested_pick}&join=flat&local=true"),
    ];

    for query in queries {
        let (status, _) = delete_entities_by_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "a Linked Entity reference must be refused: {query}");
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_delete_by_query_empty_result_returns_204() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let (status, _) = delete_entities_by_query(&c, &scope, "type=NonExistentType&local=true", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "empty result should return 204");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_delete_by_query_no_filters_returns_400() {
    let scope = TestEnvironment::from_suite("entity").await.expect("isolated test environment");
    let c = client();

    let (status, _) = delete_entities_by_query(&c, &scope, "", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no filters should return 400");
    scope.finish().await.expect("cleanup failed");
}
