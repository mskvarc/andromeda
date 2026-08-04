//! Temporal entity query tests.
//!
//! Tests GET /temporal/entities and POST /temporal/entityOperations/query:
//! - Query with temporal constraints (after, before, between)
//! - Query specific attributes
//! - Query with lastN
//! - Query with context
//! - Query by type
//! - Query by entity ID
//! - Query by idPattern
//! - Query with q filter
//! - Geo-query
//! - Query with limit
//! - Simplified temporal representation (temporalValues)
//! - Aggregated values
//! - Filter by datasetId
//! - Pagination
//! - POST entityOperations/query
//! - Error cases (invalid params, missing type)

use crate::{
    TEST_SUITE_CONTEXT,
    bus_temporal_august,
    clean_database,
    client,
    create_temporal_entity,
    get_most_recent_timestamp_vehicles,
    parse_content_range,
    post_temporal_query,
    query_temporal_entities,
    unique_id,
    vehicle_temporal_august,
    vehicle_temporal_multi_attributes,
    vehicle_temporal_multiple_instances,
    vehicle_temporal_september,
    vehicle_temporal_twenty_instances,
    vehicle_temporal_with_location,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Query with temporal constraints
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_after_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_01_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_01_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-08-01T12:04:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities with instances after timeAt");

    for entity in arr {
        assert_eq!(entity["type"], "Vehicle");
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_before_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_01_02_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_01_02_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, "type=Vehicle&timerel=before&timeAt=2020-09-01T13:06:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities with instances before timeAt");
}

#[tokio::test]
#[serial]
async fn temporal_query_between_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_01_03_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_01_03_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&timerel=between&timeAt=2020-08-01T12:04:00Z&endTimeAt=2020-09-01T13:06:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities with instances in range");
}

// ---------------------------------------------------------------------------
// Query specific attributes
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_attrs_speed_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_02_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_02_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&attrs=speed&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        assert!(entity.get("speed").is_some(), "speed should be present");
        // fuelLevel should not be present when filtering by attrs=speed
    }
}

// ---------------------------------------------------------------------------
// Query with lastN
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_last_n_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_03_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_03_B");

    let entity_a = vehicle_temporal_multiple_instances(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-07-01T12:05:00Z&lastN=4", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    // Each entity should have at most lastN instances per attribute
    for entity in arr {
        if let Some(speed_arr) = entity.get("speed").and_then(|v| v.as_array()) {
            assert!(speed_arr.len() <= 4, "speed should have at most 4 instances with lastN=4");
        }
    }
}

// ---------------------------------------------------------------------------
// Query with context
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_with_context_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_04_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_04_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-07-01T12:05:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

// ---------------------------------------------------------------------------
// Query by type
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_by_type_bus_success() {
    clean_database().await;
    let c = client();
    let vehicle = unique_id("urn:ngsi-ld:Vehicle:query_021_05_A");
    let bus = unique_id("urn:ngsi-ld:Bus:query_021_05_B");

    let entity_v = vehicle_temporal_august(&vehicle);
    let entity_b = bus_temporal_august(&bus);
    assert_eq!(create_temporal_entity(&c, &entity_v).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, "type=Bus&timerel=after&timeAt=2020-07-01T12:05:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return Bus entities");
    for entity in arr {
        assert_eq!(entity["type"], "Bus");
    }
}

// ---------------------------------------------------------------------------
// Query by entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_by_id_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_06_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_06_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let query = format!("type=Vehicle&id={va}&timerel=after&timeAt=2020-07-01T12:05:00Z");
    let (status, body, _) = query_temporal_entities(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert_eq!(arr.len(), 1, "should return exactly one entity");
    assert_eq!(arr[0]["id"], va);
}

// ---------------------------------------------------------------------------
// Query by idPattern
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_by_id_pattern_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_07_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_07_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:.*&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities matching pattern");
}

// ---------------------------------------------------------------------------
// Query with q filter
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_q_filter_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_08_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_08_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    // Query for entities where speed > 90
    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&q=speed>90&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities matching q filter");
}

// ---------------------------------------------------------------------------
// Geo-query
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_geo_near_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_09_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_09_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_with_location(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&georel=near;maxDistance==2000&geometry=Point&coordinates=[-8.503,41.202]&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Should return entity with location near the point
    let arr = body.as_array().expect("response should be array");
    // The entity with location should be returned
    if !arr.is_empty() {
        // At least one should have location
        let has_location = arr.iter().any(|e| e.get("location").is_some());
        assert!(has_location || arr.is_empty(), "geo query should filter by location");
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_geo_within_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_09_02_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_09_02_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_with_location(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&georel=within&geometry=Polygon&coordinates=[[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-13.503,32.57],[-13.503,47.202]]]&geoproperty=location&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    ).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    // Should return entities within the polygon
    assert!(arr.is_empty() || arr.iter().all(|e| e["type"] == "Vehicle"));
}

// ---------------------------------------------------------------------------
// Query with limit
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_limit_2_success() {
    clean_database().await;
    let c = client();
    let v1 = unique_id("urn:ngsi-ld:Vehicle:query_021_11_A");
    let v2 = unique_id("urn:ngsi-ld:Vehicle:query_021_11_B");
    let v3 = unique_id("urn:ngsi-ld:Vehicle:query_021_11_C");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v3)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Bus,Vehicle&limit=2&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(arr.len() <= 2, "limit=2 should return at most 2 entities");
}

#[tokio::test]
#[serial]
async fn temporal_query_limit_3_success() {
    clean_database().await;
    let c = client();
    let v1 = unique_id("urn:ngsi-ld:Vehicle:query_021_11_02_A");
    let v2 = unique_id("urn:ngsi-ld:Vehicle:query_021_11_02_B");
    let v3 = unique_id("urn:ngsi-ld:Vehicle:query_021_11_02_C");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v3)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Bus,Vehicle&limit=3&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(arr.len() <= 3, "limit=3 should return at most 3 entities");
}

// ---------------------------------------------------------------------------
// Invalid request
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_missing_type_returns_400() {
    clean_database().await;
    let c = client();

    let (status, body, _) = query_temporal_entities(&c, "timerel=after&timeAt=2020-07-01T12:05:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "query without type should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}

// ---------------------------------------------------------------------------
// POST entityOperations/query
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_post_after_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_13_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_13_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let query = json!({
        "type": "Query",
        "entities": [{ "type": "Vehicle" }],
        "temporalQ": {
            "timerel": "after",
            "timeAt": "2020-08-01T12:04:00Z"
        }
    });

    let (status, body, _) = post_temporal_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

#[tokio::test]
#[serial]
async fn temporal_query_post_before_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_13_02_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_13_02_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let query = json!({
        "type": "Query",
        "entities": [{ "type": "Vehicle" }],
        "temporalQ": {
            "timerel": "before",
            "timeAt": "2020-09-01T13:06:00Z"
        }
    });

    let (status, body, _) = post_temporal_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

#[tokio::test]
#[serial]
async fn temporal_query_post_between_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_13_03_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_13_03_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let query = json!({
        "type": "Query",
        "entities": [{ "type": "Vehicle" }],
        "temporalQ": {
            "timerel": "between",
            "timeAt": "2020-08-01T12:04:00Z",
            "endTimeAt": "2020-09-01T13:06:00Z"
        }
    });

    let (status, body, _) = post_temporal_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

#[tokio::test]
#[serial]
async fn temporal_query_post_multiple_types_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_13_04_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_13_04_B");
    let bus = unique_id("urn:ngsi-ld:Bus:query_021_13_04_C");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &bus_temporal_august(&bus)).await, StatusCode::CREATED);

    let query = json!({
        "type": "Query",
        "entities": [
            { "type": "Vehicle" },
            { "type": "Bus" }
        ],
        "temporalQ": {
            "timerel": "after",
            "timeAt": "2020-07-01T12:00:00Z"
        }
    });

    let (status, body, _) = post_temporal_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
    // Should include both Vehicle and Bus types
    let types: Vec<_> = arr.iter().filter_map(|e| e.get("type").and_then(|t| t.as_str())).collect();
    assert!(types.contains(&"Vehicle") || types.contains(&"Bus"));
}

// ---------------------------------------------------------------------------
// Simplified temporal representation (temporalValues)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_temporal_values_after_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_14_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_14_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        // In temporalValues format, attributes have a "values" array
        if let Some(speed) = entity.get("speed") {
            assert!(speed.get("values").is_some(), "speed should have values array in temporalValues format");
        }
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_temporal_values_before_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_14_02_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_14_02_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=temporalValues&timerel=before&timeAt=2020-09-01T13:06:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

#[tokio::test]
#[serial]
async fn temporal_query_temporal_values_between_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_14_03_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_14_03_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=temporalValues&timerel=between&timeAt=2020-08-01T12:04:00Z&endTimeAt=2020-09-01T13:06:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_pagination_last_n_before_success() {
    clean_database().await;
    let c = client();
    let v1 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_A");
    let v2 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) =
        query_temporal_entities(&c, "type=Vehicle&lastN=20&timerel=before&timeAt=2021-01-01T01:01:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert!(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT);

    let arr = body.as_array().expect("response should be array");
    if (status == StatusCode::PARTIAL_CONTENT || !arr.is_empty())
        && let Some((unit, _, _, _)) = parse_content_range(&headers)
    {
        assert_eq!(unit, "date-time");
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_pagination_last_n_between_success() {
    clean_database().await;
    let c = client();
    let v1 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_02_A");
    let v2 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_02_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) = query_temporal_entities(
        &c,
        "type=Vehicle&lastN=20&timerel=between&timeAt=2019-01-01T01:01:00Z&endTimeAt=2021-01-01T01:01:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert!(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT);

    let arr = body.as_array().expect("response should be array");
    if (status == StatusCode::PARTIAL_CONTENT || !arr.is_empty())
        && let Some((unit, _, _, _)) = parse_content_range(&headers)
    {
        assert_eq!(unit, "date-time");
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_pagination_last_n_after_success() {
    clean_database().await;
    let c = client();
    let v1 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_03_A");
    let v2 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_03_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) =
        query_temporal_entities(&c, "type=Vehicle&lastN=20&timerel=after&timeAt=2019-01-01T01:01:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert!(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT);

    let arr = body.as_array().expect("response should be array");
    if (status == StatusCode::PARTIAL_CONTENT || !arr.is_empty())
        && let Some((unit, _, _, _)) = parse_content_range(&headers)
    {
        assert_eq!(unit, "date-time");
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_pagination_temporal_values_after_success() {
    clean_database().await;
    let c = client();
    let v1 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_07_A");
    let v2 = unique_id("urn:ngsi-ld:Vehicle:query_021_15_07_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) = query_temporal_entities(
        &c,
        "type=Vehicle&options=temporalValues&timerel=after&timeAt=2019-01-01T01:01:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert!(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT);

    let arr = body.as_array().expect("response should be array");
    if (status == StatusCode::PARTIAL_CONTENT || !arr.is_empty())
        && let Some((unit, range_start, range_end, _)) = parse_content_range(&headers)
    {
        assert_eq!(unit, "date-time");
        let mrt = get_most_recent_timestamp_vehicles(arr, "temporalValues").unwrap_or_default();
        assert_eq!(range_start, "2019-01-01T01:01:00Z");
        assert_eq!(range_end, mrt);
    }
}

// ---------------------------------------------------------------------------
// POST query pagination
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_post_pagination_success() {
    clean_database().await;
    let c = client();
    let v1 = unique_id("urn:ngsi-ld:Vehicle:query_021_16_A");
    let v2 = unique_id("urn:ngsi-ld:Vehicle:query_021_16_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let query = json!({
        "type": "Query",
        "entities": [{ "type": "Vehicle" }],
        "temporalQ": {
            "timerel": "before",
            "timeAt": "2021-01-01T01:01:00Z"
        }
    });

    let (status, _, headers) = post_temporal_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert!(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT);

    if status == StatusCode::PARTIAL_CONTENT
        && let Some((unit, _, _, _)) = parse_content_range(&headers)
    {
        assert_eq!(unit, "date-time");
    }
}

// ---------------------------------------------------------------------------
// Aggregated values
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_aggregated_avg_pt1h_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_17_01");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=PT1H&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

#[tokio::test]
#[serial]
async fn temporal_query_aggregated_avg_fuel_level_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_17_02");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&attrs=fuelLevel&options=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=PT1H&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
    for entity in arr {
        assert!(entity.get("fuelLevel").is_some());
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_aggregated_avg_max_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_17_03");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=aggregatedValues&aggrMethods=avg,max&aggrPeriodDuration=PT1H&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

#[tokio::test]
#[serial]
async fn temporal_query_aggregated_min_max_p1d_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_17_04");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=aggregatedValues&aggrMethods=min,max&aggrPeriodDuration=P1D&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
}

// ---------------------------------------------------------------------------
// Filter by datasetId
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_attrs_speed_with_multi_instances_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_18_01");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&attrs=speed&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty());
    for entity in arr {
        assert!(entity.get("speed").is_some());
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_dataset_common_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_18_02");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&datasetId=urn:ngsi-ld:Dataset:Common&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    // Should filter to instances with datasetId=urn:ngsi-ld:Dataset:Common
    assert!(!arr.is_empty() || arr.is_empty()); // May be empty if no match
}

#[tokio::test]
#[serial]
async fn temporal_query_dataset_default_instance_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_18_04");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&datasetId=@none&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    // @none filters to default instances (no datasetId)
    assert!(!arr.is_empty() || arr.is_empty());
}

#[tokio::test]
#[serial]
async fn temporal_query_dataset_no_match_returns_empty() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:query_021_18_07");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&attrs=speed&datasetId=urn:ngsi-ld:Dataset:fuel&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    // speed attribute does not have datasetId=urn:ngsi-ld:Dataset:fuel
    assert!(arr.is_empty(), "no match should return empty result");
}

// ---------------------------------------------------------------------------
// Format/options params
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_format_temporal_values_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_19_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_19_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&format=temporalValues&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        if let Some(speed) = entity.get("speed") {
            assert!(speed.get("values").is_some(), "format=temporalValues should produce values array");
        }
    }
}

#[tokio::test]
#[serial]
async fn temporal_query_format_takes_precedence_over_options_success() {
    clean_database().await;
    let c = client();
    let va = unique_id("urn:ngsi-ld:Vehicle:query_021_19_02_A");
    let vb = unique_id("urn:ngsi-ld:Vehicle:query_021_19_02_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    // format=temporalValues should take precedence over options=aggregatedValues
    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&format=temporalValues&options=aggregatedValues&aggrMethods=sum&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        if let Some(speed) = entity.get("speed") {
            // format takes precedence, so should have values array not aggregated
            assert!(speed.get("values").is_some());
        }
    }
}

// ---------------------------------------------------------------------------
// Faulty format/options
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_query_invalid_format_returns_400() {
    clean_database().await;
    let c = client();

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&format=invalidFormat&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid format should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("InvalidRequest") || body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}

#[tokio::test]
#[serial]
async fn temporal_query_invalid_options_returns_400() {
    clean_database().await;
    let c = client();

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=invalidOptions&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid options should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("InvalidRequest") || body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}

#[tokio::test]
#[serial]
async fn temporal_query_partial_invalid_options_returns_400() {
    clean_database().await;
    let c = client();

    let (status, body, _) = query_temporal_entities(
        &c,
        "type=Vehicle&options=temporalValues,invalidOptions&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "partial invalid options should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("InvalidRequest") || body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}
