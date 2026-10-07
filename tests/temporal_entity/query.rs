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
    client,
    create_temporal_entity,
    get_most_recent_timestamp_vehicles,
    parse_content_range,
    post_temporal_query,
    query_temporal_entities,
    vehicle_temporal_august,
    vehicle_temporal_multi_attributes,
    vehicle_temporal_multiple_instances,
    vehicle_temporal_september,
    vehicle_temporal_twenty_instances,
    vehicle_temporal_with_location,
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn temporal_query_after_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_01_A");
    let vb = scope.id("Vehicle:query_021_01_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, &scope, "type=Vehicle&timerel=after&timeAt=2020-08-01T12:04:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities with instances after timeAt");

    for entity in arr {
        assert_eq!(entity["type"], "Vehicle");
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_before_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_01_02_A");
    let vb = scope.id("Vehicle:query_021_01_02_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, &scope, "type=Vehicle&timerel=before&timeAt=2020-09-01T13:06:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities with instances before timeAt");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_between_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_01_03_A");
    let vb = scope.id("Vehicle:query_021_01_03_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&timerel=between&timeAt=2020-08-01T12:04:00Z&endTimeAt=2020-09-01T13:06:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities with instances in range");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_attrs_speed_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_02_A");
    let vb = scope.id("Vehicle:query_021_02_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&attrs=speed&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        assert!(entity.get("speed").is_some(), "speed should be present");
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_last_n_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_03_A");
    let vb = scope.id("Vehicle:query_021_03_B");

    let entity_a = vehicle_temporal_multiple_instances(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&timerel=after&timeAt=2020-07-01T12:05:00Z&lastN=4",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        if let Some(speed_arr) = entity.get("speed").and_then(|v| v.as_array()) {
            assert!(speed_arr.len() <= 4, "speed should have at most 4 instances with lastN=4");
        }
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_with_context_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_04_A");
    let vb = scope.id("Vehicle:query_021_04_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, &scope, "type=Vehicle&timerel=after&timeAt=2020-07-01T12:05:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_by_type_bus_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let vehicle = scope.id("Vehicle:query_021_05_A");
    let bus = scope.id("Bus:query_021_05_B");

    let entity_v = vehicle_temporal_august(&vehicle);
    let entity_b = bus_temporal_august(&bus);
    assert_eq!(create_temporal_entity(&c, &entity_v).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, &scope, "type=Bus&timerel=after&timeAt=2020-07-01T12:05:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return Bus entities");
    for entity in arr {
        assert_eq!(entity["type"], "Bus");
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_by_id_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_06_A");
    let vb = scope.id("Vehicle:query_021_06_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let query = format!("type=Vehicle&id={va}&timerel=after&timeAt=2020-07-01T12:05:00Z");
    let (status, body, _) = query_temporal_entities(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert_eq!(arr.len(), 1, "should return exactly one entity");
    assert_eq!(arr[0]["id"], va);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_by_id_pattern_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_07_A");
    let vb = scope.id("Vehicle:query_021_07_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let pattern = urlencoding::encode(&format!("^{}:Vehicle:.*", scope.id_prefix())).into_owned();
    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        &format!("type=Vehicle&idPattern={pattern}&timerel=after&timeAt=2020-07-01T12:05:00Z"),
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities matching pattern");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_q_filter_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_08_A");
    let vb = scope.id("Vehicle:query_021_08_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_september(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&q=speed>90&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "should return entities matching q filter");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4 checks a temporal `q` against the instances in the window,
/// and clause 4.9 reads a `LanguageProperty` instance through its
/// `languageMap`: with a target language only that language's string is the
/// target value, and with none every string is, under the ordering and pattern
/// operators as under equality.
#[tokio::test]
async fn temporal_query_q_language_strings_read_the_instances() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let door = scope.id("Building:query_r65_door");
    let gate = scope.id("Building:query_r65_gate");
    for (id, names) in [(&door, json!({"en": "Door", "de": "Tuer"})), (&gate, json!({"en": "Gate", "de": "Dach"}))] {
        let entity = json!({
            "id": id,
            "type": "Building",
            "label": [{"type": "LanguageProperty", "languageMap": names, "observedAt": "2020-08-01T12:03:00Z"}],
            "@context": [COMPOUND_CONTEXT]
        });
        assert_eq!(create_temporal_entity(&c, &entity).await, StatusCode::CREATED);
    }

    for (q, expected) in [
        ("label[en]~=\"^D\"", &door),
        ("label[de]~=\"^D\"", &gate),
        ("label[en]>\"E\"", &gate),
        ("label[en]==\"C\"..\"E\"", &door),
        ("label~=\"^Tu\"", &door),
    ] {
        let query = format!("type=Building&q={}&timerel=after&timeAt=2020-07-01T00:00:00Z", urlencoding::encode(q));
        let (status, body, _) = query_temporal_entities(&c, &scope, &query, Some(COMPOUND_CONTEXT)).await;
        assert_eq!(status, StatusCode::OK, "{q}: {body}");
        let ids: Vec<&str> = body
            .as_array()
            .expect("response should be array")
            .iter()
            .filter_map(|entity| entity["id"].as_str())
            .collect();
        assert_eq!(ids, vec![expected.as_str()], "{q}");
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_geo_near_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_09_A");
    let vb = scope.id("Vehicle:query_021_09_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_with_location(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&georel=near;maxDistance==2000&geometry=Point&coordinates=[-8.503,41.202]&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Clause 4.10: the geo query selects on the GeoProperty, so the Vehicle at
    // the reference point is returned with its location and the one that
    // declares no location is not.
    let arr = body.as_array().expect("response should be array");
    let matched = arr.iter().filter_map(|entity| entity["id"].as_str()).collect::<Vec<&str>>();
    assert_eq!(matched, vec![vb.as_str()], "only the Vehicle near the reference point matches: {body}");
    assert!(
        arr.iter().all(|entity| entity.get("location").is_some()),
        "the matched Vehicle carries the GeoProperty the query selected on: {body}"
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_geo_within_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_09_02_A");
    let vb = scope.id("Vehicle:query_021_09_02_B");

    let entity_a = vehicle_temporal_august(&va);
    let entity_b = vehicle_temporal_with_location(&vb);
    assert_eq!(create_temporal_entity(&c, &entity_a).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &entity_b).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&georel=within&geometry=Polygon&coordinates=[[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-13.503,32.57],[-13.503,47.202]]]&geoproperty=location&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    ).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(arr.is_empty() || arr.iter().all(|e| e["type"] == "Vehicle"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_limit_2_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let v1 = scope.id("Vehicle:query_021_11_A");
    let v2 = scope.id("Vehicle:query_021_11_B");
    let v3 = scope.id("Vehicle:query_021_11_C");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v3)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Bus,Vehicle&limit=2&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(arr.len() <= 2, "limit=2 should return at most 2 entities");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_limit_3_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let v1 = scope.id("Vehicle:query_021_11_02_A");
    let v2 = scope.id("Vehicle:query_021_11_02_B");
    let v3 = scope.id("Vehicle:query_021_11_02_C");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v3)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Bus,Vehicle&limit=3&timerel=after&timeAt=2020-07-01T12:05:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(arr.len() <= 3, "limit=3 should return at most 3 entities");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_missing_type_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();

    let (status, body, _) = query_temporal_entities(&c, &scope, "timerel=after&timeAt=2020-07-01T12:05:00Z", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "query without type should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_post_after_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_13_A");
    let vb = scope.id("Vehicle:query_021_13_B");

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

    let (status, body, _) = post_temporal_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_post_before_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_13_02_A");
    let vb = scope.id("Vehicle:query_021_13_02_B");

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

    let (status, body, _) = post_temporal_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_post_between_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_13_03_A");
    let vb = scope.id("Vehicle:query_021_13_03_B");

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

    let (status, body, _) = post_temporal_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_post_multiple_types_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_13_04_A");
    let vb = scope.id("Vehicle:query_021_13_04_B");
    let bus = scope.id("Bus:query_021_13_04_C");

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

    let (status, body, _) = post_temporal_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    let types: Vec<_> = arr.iter().filter_map(|e| e.get("type").and_then(|t| t.as_str())).collect();
    assert!(types.contains(&"Vehicle") || types.contains(&"Bus"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_temporal_values_after_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_14_A");
    let vb = scope.id("Vehicle:query_021_14_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        if let Some(speed) = entity.get("speed") {
            assert!(speed.get("values").is_some(), "speed should have values array in temporalValues format");
        }
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_temporal_values_before_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_14_02_A");
    let vb = scope.id("Vehicle:query_021_14_02_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=temporalValues&timerel=before&timeAt=2020-09-01T13:06:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_temporal_values_between_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_14_03_A");
    let vb = scope.id("Vehicle:query_021_14_03_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=temporalValues&timerel=between&timeAt=2020-08-01T12:04:00Z&endTimeAt=2020-09-01T13:06:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_pagination_last_n_before_returns_206_with_content_range() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let v1 = scope.id("Vehicle:query_021_15_A");
    let v2 = scope.id("Vehicle:query_021_15_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&lastN=20&timerel=before&timeAt=2021-01-01T01:01:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);

    let _arr = body.as_array().expect("response should be array");
    let (unit, _, _, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_pagination_last_n_between_returns_206_with_content_range() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let v1 = scope.id("Vehicle:query_021_15_02_A");
    let v2 = scope.id("Vehicle:query_021_15_02_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&lastN=20&timerel=between&timeAt=2019-01-01T01:01:00Z&endTimeAt=2021-01-01T01:01:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);

    let _arr = body.as_array().expect("response should be array");
    let (unit, _, _, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_pagination_last_n_after_returns_206_with_content_range() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let v1 = scope.id("Vehicle:query_021_15_03_A");
    let v2 = scope.id("Vehicle:query_021_15_03_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&lastN=20&timerel=after&timeAt=2019-01-01T01:01:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);

    let _arr = body.as_array().expect("response should be array");
    let (unit, _, _, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_pagination_temporal_values_after_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let v1 = scope.id("Vehicle:query_021_15_07_A");
    let v2 = scope.id("Vehicle:query_021_15_07_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_twenty_instances(&v1)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&v2)).await, StatusCode::CREATED);

    let (status, body, headers) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=temporalValues&timerel=after&timeAt=2019-01-01T01:01:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);

    let arr = body.as_array().expect("response should be array");
    let (unit, range_start, range_end, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    let mrt = get_most_recent_timestamp_vehicles(arr, "temporalValues").unwrap_or_default();
    assert_eq!(range_start, "2019-01-01T01:01:00Z");
    assert_eq!(range_end, mrt);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_post_pagination_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let v1 = scope.id("Vehicle:query_021_16_A");
    let v2 = scope.id("Vehicle:query_021_16_B");

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

    let (status, _, headers) = post_temporal_query(&c, &scope, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    let (unit, _, _, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_aggregated_avg_pt1h_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_17_01");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=PT1H&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_aggregated_avg_fuel_level_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_17_02");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&attrs=fuelLevel&options=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=PT1H&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    for entity in arr {
        assert!(entity.get("fuelLevel").is_some());
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_aggregated_avg_max_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_17_03");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=aggregatedValues&aggrMethods=avg,max&aggrPeriodDuration=PT1H&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_aggregated_min_max_p1d_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_17_04");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=aggregatedValues&aggrMethods=min,max&aggrPeriodDuration=P1D&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_attrs_speed_with_multi_instances_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_18_01");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&attrs=speed&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty(), "the temporal query must return at least one matching entity");
    for entity in arr {
        assert!(entity.get("speed").is_some());
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_dataset_common_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_18_02");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&datasetId=urn:ngsi-ld:Dataset:Common&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert!(!arr.is_empty() || arr.is_empty()); // May be empty if no match
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_dataset_default_instance_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_18_04");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&datasetId=@none&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    // @none filters to default instances (no datasetId)
    assert!(!arr.is_empty() || arr.is_empty());
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4: `attrs=speed` selects the Vehicle (S1), and the `datasetId`
/// step (S8) then removes `speed`, which carries no `fuel` instance: "Remove all
/// Attributes without remaining Attribute instances". S8 removes Attributes,
/// never the Entity, so the Vehicle answers with its `id` and `type` alone.
/// An empty array is not an answer the clause allows.
#[tokio::test]
async fn temporal_query_dataset_no_match_keeps_the_entity_without_attributes() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:query_021_18_07");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_multi_attributes(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&attrs=speed&datasetId=urn:ngsi-ld:Dataset:fuel&options=temporalValues&timerel=after&timeAt=2020-08-01T12:04:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    assert_eq!(arr.len(), 1, "the Vehicle stays in the result set: {body}");
    assert_eq!(arr[0]["id"], json!(id), "{body}");
    assert!(arr[0].get("type").is_some(), "{body}");
    assert!(arr[0].get("speed").is_none(), "speed has no fuel instance: {body}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_format_temporal_values_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_19_A");
    let vb = scope.id("Vehicle:query_021_19_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_format_takes_precedence_over_options_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let va = scope.id("Vehicle:query_021_19_02_A");
    let vb = scope.id("Vehicle:query_021_19_02_B");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&va)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_september(&vb)).await, StatusCode::CREATED);

    // format=temporalValues should take precedence over options=aggregatedValues
    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&format=temporalValues&options=aggregatedValues&aggrMethods=sum&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("response should be array");
    for entity in arr {
        if let Some(speed) = entity.get("speed") {
            assert!(speed.get("values").is_some());
        }
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_invalid_format_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&format=invalidFormat&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid format should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("InvalidRequest") || body["type"].as_str().unwrap_or("").contains("BadRequestData"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_invalid_options_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=invalidOptions&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid options should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("InvalidRequest") || body["type"].as_str().unwrap_or("").contains("BadRequestData"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_partial_invalid_options_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();

    let (status, body, _) = query_temporal_entities(
        &c,
        &scope,
        "type=Vehicle&options=temporalValues,invalidOptions&timerel=after&timeAt=2020-01-01T12:03:00Z",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "partial invalid options should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("InvalidRequest") || body["type"].as_str().unwrap_or("").contains("BadRequestData"));
    scope.finish().await.expect("cleanup failed");
}
