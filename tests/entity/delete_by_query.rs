//! Batch entity deletion tests.
//!
//! Tests entity deletion by query operations:
//! - Delete by type
//! - Delete by ID list
//! - Delete by ID pattern
//! - Delete by q filter
//! - Empty result returns 204
//! - No filters returns 400

use crate::{
    TEST_SUITE_CONTEXT,
    building_simple_attributes,
    clean_database,
    client,
    create_entity_json,
    delete_entities_by_query,
    get_entity,
    unique_id,
    vehicle_simple_attributes,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Delete by type
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_by_query_by_type_success() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:dq_type_1");
    let id2 = unique_id("urn:ngsi-ld:Building:dq_type_2");
    let id3 = unique_id("urn:ngsi-ld:Building:dq_type_3");
    let id_vehicle = unique_id("urn:ngsi-ld:Vehicle:dq_type_v");

    for id in [&id1, &id2, &id3] {
        let entity = building_simple_attributes(id);
        let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let vehicle = vehicle_simple_attributes(&id_vehicle);
    let status = create_entity_json(&c, &vehicle, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entities_by_query(&c, "type=Building&local=true", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by type should return 204");

    // Verify buildings are gone
    for id in [&id1, &id2, &id3] {
        let (status, _) = get_entity(&c, id, "", "application/json", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "building {id} should be deleted");
    }

    // Verify vehicle remains
    let (status, _) = get_entity(&c, &id_vehicle, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK, "vehicle should still exist");
}

// ---------------------------------------------------------------------------
// Delete by ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_by_query_by_id_success() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:dq_id_1");
    let id2 = unique_id("urn:ngsi-ld:Building:dq_id_2");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let ids = format!("{id1},{id2}");
    let query = format!("id={}&local=true", urlencoding::encode(&ids));
    let (status, _) = delete_entities_by_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by id should return 204");

    for id in [&id1, &id2] {
        let (status, _) = get_entity(&c, id, "", "application/json", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "entity {id} should be deleted");
    }
}

// ---------------------------------------------------------------------------
// Delete by ID pattern
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_by_query_by_id_pattern_success() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:dq_pat_match1");
    let id2 = unique_id("urn:ngsi-ld:Building:dq_pat_match2");
    let id_other = unique_id("urn:ngsi-ld:Building:dq_pat_other");

    for id in [&id1, &id2, &id_other] {
        let entity = building_simple_attributes(id);
        let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let pattern = urlencoding::encode("urn:ngsi-ld:Building:dq_pat_match.*");
    let query = format!("idPattern={pattern}&local=true");
    let (status, _) = delete_entities_by_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by idPattern should return 204");

    for id in [&id1, &id2] {
        let (status, _) = get_entity(&c, id, "", "application/json", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "matched entity {id} should be deleted");
    }

    let (status, _) = get_entity(&c, &id_other, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK, "unmatched entity should remain");
}

// ---------------------------------------------------------------------------
// Delete by q filter
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_by_query_by_q_filter_success() {
    clean_database().await;
    let c = client();

    let id_high = unique_id("urn:ngsi-ld:Building:dq_q_high");
    let id_low = unique_id("urn:ngsi-ld:Building:dq_q_low");

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
    let (status, _) = delete_entities_by_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete by q filter should return 204");

    let (status, _) = get_entity(&c, &id_high, "", "application/json", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "high value entity should be deleted");

    let (status, _) = get_entity(&c, &id_low, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK, "low value entity should remain");
}

// ---------------------------------------------------------------------------
// Empty result returns 204
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_by_query_empty_result_returns_204() {
    clean_database().await;
    let c = client();

    let (status, _) = delete_entities_by_query(&c, "type=NonExistentType&local=true", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "empty result should return 204");
}

// ---------------------------------------------------------------------------
// No filters returns 400
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_by_query_no_filters_returns_400() {
    clean_database().await;
    let c = client();

    let (status, _) = delete_entities_by_query(&c, "", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no filters should return 400");
}
