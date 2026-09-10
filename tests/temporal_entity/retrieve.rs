//! Temporal entity retrieve tests.
//!
//! Tests GET /temporal/entities/{entityId} operations:
//! - Basic retrieve
//! - Retrieve with context
//! - Retrieve specific attributes
//! - Temporal queries (timerel: after, before, between)
//! - lastN parameter
//! - Simplified temporal representation (temporalValues)
//! - Aggregated values
//! - `LanguageProperty`, `JsonProperty`, `VocabProperty` representations
//! - Pagination
//! - `DeletedAt` handling
//! - Error cases (invalid ID, non-existent entity, invalid temporal params)

use crate::{
    TEST_SUITE_CONTEXT,
    clean_database,
    client,
    create_temporal_entity,
    delete_entity_attr,
    get_most_recent_timestamp,
    get_temporal_entity,
    parse_content_range,
    unique_id,
    vehicle_temporal_august,
    vehicle_temporal_different_types,
    vehicle_temporal_json_property,
    vehicle_temporal_language_property,
    vehicle_temporal_multiple_instances,
    vehicle_temporal_twenty_instances,
    vehicle_temporal_vocab_property,
    vehicle_temporal_with_scope,
};
use reqwest::StatusCode;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Basic retrieve
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_basic_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_01");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create temporal entity should succeed");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK, "retrieve should return 200");
    assert_eq!(body["id"], id);
    assert_eq!(body["type"], "Vehicle");
    assert!(body.get("speed").is_some(), "speed attribute should exist");
    assert!(body.get("fuelLevel").is_some(), "fuelLevel attribute should exist");
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_with_context_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_02");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body.get("speed").is_some());
}

// ---------------------------------------------------------------------------
// Retrieve specific attributes
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_attrs_fuel_level_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_03_01");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "attrs=fuelLevel", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("fuelLevel").is_some(), "fuelLevel should be present");
    assert!(body.get("speed").is_none(), "speed should not be present when filtering by attrs");
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_attrs_multiple_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_03_02");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "attrs=fuelLevel,speed", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("fuelLevel").is_some(), "fuelLevel should be present");
    assert!(body.get("speed").is_some(), "speed should be present");
}

// ---------------------------------------------------------------------------
// Temporal queries (timerel)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_timerel_after_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_04_01");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "timerel=after&timeAt=2020-08-01T13:03:00Z",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Should only return instances after 13:03:00
    if let Some(fuel_arr) = body.get("fuelLevel").and_then(|v| v.as_array()) {
        for instance in fuel_arr {
            let ts = instance.get("observedAt").and_then(|v| v.as_str()).unwrap_or("");
            assert!(ts >= "2020-08-01T13:03:00Z", "fuelLevel instance should be after timeAt");
        }
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_timerel_before_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_04_02");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "timerel=before&timeAt=2020-08-01T12:05:00Z",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Should only return instances before 12:05:00
    if let Some(speed_arr) = body.get("speed").and_then(|v| v.as_array()) {
        for instance in speed_arr {
            let ts = instance.get("observedAt").and_then(|v| v.as_str()).unwrap_or("9999");
            assert!(ts <= "2020-08-01T12:05:00Z", "speed instance should be before timeAt");
        }
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_timerel_between_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_04_03");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T13:00:00Z",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Should only return instances between the time range
    if let Some(speed_arr) = body.get("speed").and_then(|v| v.as_array()) {
        for instance in speed_arr {
            let ts = instance.get("observedAt").and_then(|v| v.as_str()).unwrap_or("");
            assert!(("2020-08-01T12:00:00Z"..="2020-08-01T13:00:00Z").contains(&ts), "instance should be in range");
        }
    }
}

// ---------------------------------------------------------------------------
// lastN parameter
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_last_n_4_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_05_01");

    let entity = vehicle_temporal_multiple_instances(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "lastN=4", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // lastN=4 should return at most 4 instances per attribute
    if let Some(speed_arr) = body.get("speed").and_then(|v| v.as_array()) {
        assert!(speed_arr.len() <= 4, "speed should have at most 4 instances with lastN=4");
    }
    if let Some(fuel_arr) = body.get("fuelLevel").and_then(|v| v.as_array()) {
        assert!(fuel_arr.len() <= 4, "fuelLevel should have at most 4 instances with lastN=4");
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_last_n_7_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_05_02");

    let entity = vehicle_temporal_multiple_instances(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "lastN=7", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    if let Some(speed_arr) = body.get("speed").and_then(|v| v.as_array()) {
        assert!(speed_arr.len() <= 7, "speed should have at most 7 instances with lastN=7");
    }
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_invalid_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, body, _) = get_temporal_entity(&c, "invalidUri", "", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_nonexistent_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, body, _) = get_temporal_entity(&c, "urn:ngsi-ld:Vehicle:unknownEntity", "", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    assert!(body["type"].as_str().unwrap_or("").contains("ResourceNotFound"));
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_unknown_attrs_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_08");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "attrs=unknownAttribute", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown attribute should return 404");
    assert!(body["type"].as_str().unwrap_or("").contains("ResourceNotFound"));
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_after_missing_timeat_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_09_01");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "timerel=after", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "after without timeAt should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_before_missing_timeat_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_09_02");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "timerel=before", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "before without timeAt should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_between_missing_endtimeat_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_09_03");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "timerel=between&timeAt=2020-08-01T12:00:00Z", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "between without endTimeAt should return 400");
    assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
}

// ---------------------------------------------------------------------------
// Simplified temporal representation (temporalValues)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_temporal_values_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_10");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "options=temporalValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // In temporalValues format, attributes have a "values" array of [value, timestamp] pairs
    if let Some(speed) = body.get("speed") {
        assert!(speed.get("values").is_some(), "speed should have values array in temporalValues format");
    }
}

// ---------------------------------------------------------------------------
// Aggregated values
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_aggregated_avg_pt1h_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_11_01");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "options=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=PT1H",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some());
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_aggregated_avg_fuel_level_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_11_02");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "attrs=fuelLevel&options=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=PT1H",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("fuelLevel").is_some());
    assert!(body.get("speed").is_none());
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_aggregated_avg_max_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_11_03");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "options=aggregatedValues&aggrMethods=avg,max&aggrPeriodDuration=PT1H",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some());
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_aggregated_min_max_p1d_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_11_04");

    let entity = vehicle_temporal_august(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "options=aggregatedValues&aggrMethods=min,max&aggrPeriodDuration=P1D",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some());
}

// ---------------------------------------------------------------------------
// LanguageProperty representation
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_language_property_normalized_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_12_01");

    let entity = vehicle_temporal_language_property(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // Check LanguageProperty structure
    if let Some(name_arr) = body.get("name").and_then(|v| v.as_array()) {
        assert!(!name_arr.is_empty(), "the retrieved LanguageProperty must carry at least one instance");
        let first = &name_arr[0];
        assert_eq!(first["type"], "LanguageProperty");
        assert!(first.get("languageMap").is_some());
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_language_property_simplified_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_12_02");

    let entity = vehicle_temporal_language_property(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "options=temporalValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // In temporalValues, LanguageProperty uses languageMaps array
    if let Some(name) = body.get("name") {
        assert!(name.get("languageMaps").is_some() || name.get("values").is_some());
    }
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_pagination_basic_returns_206() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_13_01");

    let entity = vehicle_temporal_twenty_instances(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, headers) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT, "pagination must return 206");
    let (unit, _, _, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    assert!(body.get("speed").is_some());
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_pagination_last_n_returns_206_with_content_range() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_13_02");

    let entity = vehicle_temporal_twenty_instances(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, headers) = get_temporal_entity(&c, &id, "lastN=20", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT, "partial lastN result must return 206");
    let (unit, _, _, size) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    assert_eq!(size, "20");
    assert!(body.get("speed").is_some());
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_pagination_last_n_before_returns_206_with_content_range() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_13_03");

    let entity = vehicle_temporal_twenty_instances(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, headers) = get_temporal_entity(
        &c,
        &id,
        "lastN=20&timerel=before&timeAt=2021-01-01T01:01:00Z",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT, "partial bounded lastN result must return 206");
    let (unit, range_start, _, size) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    assert_eq!(range_start, "2021-01-01T01:01:00Z");
    assert_eq!(size, "20");
    assert!(body.get("speed").is_some());
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_pagination_after_returns_206() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_13_08");

    let entity = vehicle_temporal_twenty_instances(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, headers) = get_temporal_entity(
        &c,
        &id,
        "timerel=after&timeAt=2019-01-01T01:01:00Z",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT, "temporal pagination must return 206");
    let (unit, range_start, range_end, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    let mrt = get_most_recent_timestamp(&body, "").unwrap_or_default();
    assert_eq!(range_start, "2019-01-01T01:01:00Z");
    assert_eq!(range_end, mrt);
    assert!(body.get("speed").is_some());
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_pagination_temporal_values_after_returns_206() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_13_09");

    let entity = vehicle_temporal_twenty_instances(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, headers) = get_temporal_entity(
        &c,
        &id,
        "options=temporalValues&timerel=after&timeAt=2019-01-01T01:01:00Z",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT, "temporalValues pagination must return 206");
    let (unit, range_start, range_end, _) = parse_content_range(&headers).expect("partial temporal response must include Content-Range");
    assert_eq!(unit, "DateTime", "clause 6.3.10: the range unit shall be equal to \"DateTime\"");
    let mrt = get_most_recent_timestamp(&body, "temporalValues").unwrap_or_default();
    assert_eq!(range_start, "2019-01-01T01:01:00Z");
    assert_eq!(range_end, mrt);
    assert!(body.get("speed").is_some());
}

// ---------------------------------------------------------------------------
// JsonProperty representation
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_json_property_normalized_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_15_01");

    let entity = vehicle_temporal_json_property(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    if let Some(config_arr) = body.get("config").and_then(|v| v.as_array()) {
        assert!(!config_arr.is_empty(), "the retrieved JsonProperty must carry at least one instance");
        let first = &config_arr[0];
        assert_eq!(first["type"], "JsonProperty");
        assert!(first.get("json").is_some());
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_json_property_simplified_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_15_02");

    let entity = vehicle_temporal_json_property(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "options=temporalValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // In temporalValues, JsonProperty uses jsons array
    if let Some(config) = body.get("config") {
        assert!(config.get("jsons").is_some() || config.get("values").is_some());
    }
}

// ---------------------------------------------------------------------------
// VocabProperty representation
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_vocab_property_normalized_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_16_01");

    let entity = vehicle_temporal_vocab_property(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    if let Some(cat_arr) = body.get("category").and_then(|v| v.as_array()) {
        assert!(!cat_arr.is_empty(), "the retrieved VocabProperty must carry at least one instance");
        let first = &cat_arr[0];
        assert_eq!(first["type"], "VocabProperty");
        assert!(first.get("vocab").is_some());
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_vocab_property_simplified_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_16_02");

    let entity = vehicle_temporal_vocab_property(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "options=temporalValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // In temporalValues, VocabProperty uses vocabs array
    if let Some(cat) = body.get("category") {
        assert!(cat.get("vocabs").is_some() || cat.get("values").is_some());
    }
}

// ---------------------------------------------------------------------------
// DeletedAt temporal property
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_deleted_at_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_17_01");

    let entity = vehicle_temporal_different_types(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete the fuelLevel attribute
    let del_status = delete_entity_attr(&c, &id, "fuelLevel", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT);

    // Retrieve with timeproperty=deletedAt
    let (status, body, _) = get_temporal_entity(&c, &id, "timeproperty=deletedAt", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // fuelLevel should have deletedAt
    if let Some(fuel_arr) = body.get("fuelLevel").and_then(|v| v.as_array())
        && !fuel_arr.is_empty()
    {
        assert!(fuel_arr[0].get("deletedAt").is_some(), "deletedAt should be present on fuelLevel");
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_deleted_at_relationship_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_17_02");

    let entity = vehicle_temporal_different_types(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete the isParkedIn relationship
    let del_status = delete_entity_attr(&c, &id, "isParkedIn", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT);

    // Retrieve with timeproperty=deletedAt
    let (status, body, _) = get_temporal_entity(&c, &id, "timeproperty=deletedAt", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    if let Some(parked_arr) = body.get("isParkedIn").and_then(|v| v.as_array())
        && !parked_arr.is_empty()
    {
        assert!(parked_arr[0].get("deletedAt").is_some(), "deletedAt should be present on isParkedIn");
    }
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_deleted_at_language_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_17_03");

    let entity = vehicle_temporal_different_types(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete the name LanguageProperty
    let del_status = delete_entity_attr(&c, &id, "name", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT);

    // Retrieve with timeproperty=deletedAt
    let (status, body, _) = get_temporal_entity(&c, &id, "timeproperty=deletedAt", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    if let Some(name_arr) = body.get("name").and_then(|v| v.as_array())
        && !name_arr.is_empty()
    {
        assert!(name_arr[0].get("deletedAt").is_some(), "deletedAt should be present on name");
    }
}

// ---------------------------------------------------------------------------
// DeletedAt with temporal values
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_deleted_at_temporal_values_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_18_01");

    let entity = vehicle_temporal_different_types(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let del_status = delete_entity_attr(&c, &id, "fuelLevel", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "timeproperty=deletedAt&options=temporalValues",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // In temporalValues with deletedAt, the simplified array should contain deletion timestamp
    if let Some(fuel) = body.get("fuelLevel")
        && let Some(values) = fuel.get("values").and_then(|v| v.as_array())
        && !values.is_empty()
    {
        // Second element of first entry should be the deletedAt timestamp
        assert!(values[0].get(1).is_some());
    }
}

// ---------------------------------------------------------------------------
// DeletedAt on scope
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_retrieve_deleted_at_scope_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_19");

    let entity = vehicle_temporal_with_scope(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let del_status = delete_entity_attr(&c, &id, "scope", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT);

    let (status, body, _) = get_temporal_entity(&c, &id, "timeproperty=deletedAt", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // Clause 4.18 reifies the Scope in the temporal representation, and clause
    // 4.5.7 records a deleted instance "with its value set to the URI
    // "urn:ngsi-ld:null" and the deletedAt Temporal Property set". A
    // `timeproperty=deletedAt` window selects the instances that carry that
    // axis, so the removal is the one instance the Scope answers with.
    let scope = body.get("scope").expect("clause 4.18 reifies the scope in the temporal representation");
    let instances = scope.as_array().cloned().unwrap_or_else(|| vec![scope.clone()]);
    assert_eq!(instances.len(), 1, "the deletedAt window selects the removal alone: {scope}");
    assert_eq!(instances[0]["value"], "urn:ngsi-ld:null", "{scope}");
    assert!(instances[0].get("deletedAt").is_some(), "deletedAt should be present on scope: {scope}");
}

#[tokio::test]
#[serial]
async fn temporal_retrieve_deleted_at_temporal_values_scope_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:retrieve_020_20");

    let entity = vehicle_temporal_with_scope(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let del_status = delete_entity_attr(&c, &id, "scope", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT);

    let (status, body, _) = get_temporal_entity(
        &c,
        &id,
        "timeproperty=deletedAt&options=temporalValues",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Clause 4.5.9 collapses the reified Scope into `[value, instant]` pairs
    // like any other attribute-shaped member, and a deleted instance pairs the
    // NGSI-LD Null with the `deletedAt` that stands in for the Temporal
    // Property it never observed.
    let scope = body.get("scope").expect("clause 4.18 reifies the scope in the temporal representation");
    let values = scope
        .get("values")
        .and_then(|values| values.as_array())
        .expect("a temporalValues member holds values");
    assert_eq!(values.len(), 1, "the deletedAt window selects the removal alone: {scope}");
    assert_eq!(values[0][0], "urn:ngsi-ld:null", "{scope}");
    assert!(values[0].get(1).is_some(), "the deletion instant is the pair's second element: {scope}");
}
