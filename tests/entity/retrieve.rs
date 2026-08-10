//! Entity retrieve integration tests.

use crate::{
    COMPOUND_CONTEXT,
    TEST_SUITE_CONTEXT,
    building_language_property,
    building_observation_space,
    building_operation_space,
    building_relationship,
    building_simple_attributes,
    building_two_geometry_attributes,
    city_minimal,
    city_relationship,
    clean_database,
    client,
    country_minimal,
    create_entity,
    create_entity_json,
    get_entity,
    unique_id,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Retrieve by ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_by_id_returns_entity() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:retrieve_id");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "", "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert_eq!(body["type"], "Building");
    assert!(body.get("name").is_some());
    assert!(body.get("airQualityLevel").is_some());
}

// ---------------------------------------------------------------------------
// Retrieve with attrs filter
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_with_attrs_filter_returns_only_requested() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:retrieve_attrs");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let query = "attrs=https://ngsi-ld-test-suite/context%23airQualityLevel,https://ngsi-ld-test-suite/context%23subCategory";
    let (status, body) = get_entity(&client, &id, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body.get("airQualityLevel").is_some());
    assert!(body.get("subCategory").is_some());
    assert!(body.get("name").is_none());
}

// ---------------------------------------------------------------------------
// Invalid entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
async fn entity_retrieve_invalid_id_returns_bad_request() {
    let client = client();
    let (status, body) = get_entity(&client, "thisisaninvaliduri", "", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
}

// ---------------------------------------------------------------------------
// Entity not found
// ---------------------------------------------------------------------------

#[tokio::test]
async fn entity_retrieve_not_found_returns_404() {
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:not_found");
    let (status, body) = get_entity(&client, &id, "", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["type"].as_str().unwrap().contains("ResourceNotFound"));
}

#[tokio::test]
#[serial]
async fn entity_retrieve_unknown_attribute_returns_404() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:unknown_attr");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "attrs=property_not_found", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["type"].as_str().unwrap().contains("ResourceNotFound"));
}

// ---------------------------------------------------------------------------
// Simplified representation
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_key_values_returns_simplified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:kv");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "options=keyValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body["name"].is_string(), "name should be a simple string");
}

// ---------------------------------------------------------------------------
// GeoJSON representation
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_geojson_simplified_returns_feature() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:geojson_simple");

    assert_eq!(create_entity(&client, &building_two_geometry_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "options=keyValues", "application/geo+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "Feature");
    assert!(body.get("geometry").is_some());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_geojson_normalized_returns_feature_with_properties() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:geojson_norm");

    assert_eq!(create_entity(&client, &building_two_geometry_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "", "application/geo+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "Feature");
    assert!(body.get("geometry").is_some());
    assert!(body.get("properties").is_some());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_geojson_geometry_property_uses_specified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:geojson_geoprop");

    assert_eq!(create_entity(&client, &building_two_geometry_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &id,
        "geometryProperty=observationSpace",
        "application/geo+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "Feature");
    assert!(body.get("geometry").is_some());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_geojson_nonexistent_geometry_property_returns_null_geometry() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:geojson_null");

    assert_eq!(create_entity(&client, &building_two_geometry_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &id,
        "geometryProperty=operationSpace",
        "application/geo+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "Feature");
    assert!(body["geometry"].is_null());
}

// ---------------------------------------------------------------------------
// JSON-LD context resolution
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_json_empty_context_returns_expanded_terms() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:json_empty_ctx");

    let entity = json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Test Building"
        }
    });
    assert_eq!(create_entity_json(&client, &entity, TEST_SUITE_CONTEXT).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(
        body.get("https://ngsi-ld-test-suite/context#name").is_some() || body.get("name").is_some(),
        "should contain name attribute"
    );
}

#[tokio::test]
#[serial]
async fn entity_retrieve_json_with_context_returns_compacted_terms() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:json_ctx");

    let entity = json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Test Building"
        }
    });
    assert_eq!(create_entity_json(&client, &entity, TEST_SUITE_CONTEXT).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "", "application/json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body.get("name").is_some());
}

// ---------------------------------------------------------------------------
// Language filter
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_lang_fr_returns_french() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:lang_fr");

    assert_eq!(create_entity(&client, &building_language_property(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "lang=fr", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_lang_multi_no_ranked_returns_first_match() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:lang_multi");

    assert_eq!(create_entity(&client, &building_language_property(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "lang=fr-CH,fr", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_lang_any_returns_any_language() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:lang_any");

    assert_eq!(create_entity(&client, &building_language_property(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "lang=*", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_lang_ranked_preferences_returns_best_match() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:lang_ranked");

    assert_eq!(create_entity(&client, &building_language_property(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &id,
        "lang=fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_lang_fr_simplified_returns_simplified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:lang_fr_kv");

    assert_eq!(create_entity(&client, &building_language_property(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "lang=fr&options=keyValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_lang_any_simplified_returns_simplified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:lang_any_kv");

    assert_eq!(create_entity(&client, &building_language_property(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "lang=*&options=keyValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

// ---------------------------------------------------------------------------
// observationSpace geoProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_observation_space_simplified_present() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:obs_space_kv");

    assert_eq!(create_entity(&client, &building_observation_space(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "options=keyValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body.get("observationSpace").is_some());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_observation_space_normalized_is_geoproperty() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:obs_space_norm");

    assert_eq!(create_entity(&client, &building_observation_space(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body.get("observationSpace").is_some());
    assert_eq!(body["observationSpace"]["type"], "GeoProperty");
}

// ---------------------------------------------------------------------------
// operationSpace geoProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_operation_space_simplified_present() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:op_space_kv");

    assert_eq!(create_entity(&client, &building_operation_space(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "options=keyValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body.get("operationSpace").is_some());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_operation_space_normalized_is_geoproperty() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:op_space_norm");

    assert_eq!(create_entity(&client, &building_operation_space(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body.get("operationSpace").is_some());
    assert_eq!(body["operationSpace"]["type"], "GeoProperty");
}

// ---------------------------------------------------------------------------
// One-level linked entities
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_inline_linked_returns_nested_entity() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:inline_linked");
    let city_id = unique_id("urn:ngsi-ld:City:inline_linked");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=inline&joinLevel=1",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], building_id);
    assert!(body["locatedAt"].get("entity").is_some());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_flat_linked_returns_array() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:flat_linked");
    let city_id = unique_id("urn:ngsi-ld:City:flat_linked");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &building_id, "join=flat&joinLevel=1", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("flat join should return array");
    assert!(arr.len() >= 2, "should contain building and city");
}

#[tokio::test]
#[serial]
async fn entity_retrieve_inline_linked_simplified_returns_nested() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:inline_kv");
    let city_id = unique_id("urn:ngsi-ld:City:inline_kv");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=inline&joinLevel=1&options=keyValues",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], building_id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_flat_linked_simplified_returns_array() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:flat_kv");
    let city_id = unique_id("urn:ngsi-ld:City:flat_kv");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=flat&joinLevel=1&options=keyValues",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("flat join should return array");
    assert!(arr.len() >= 2);
}

// ---------------------------------------------------------------------------
// Inline linked entity with sysAttrs
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_inline_linked_sys_attrs_includes_created_at() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:inline_sys");
    let city_id = unique_id("urn:ngsi-ld:City:inline_sys");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=inline&joinLevel=1&options=sysAttrs",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], building_id);
    assert!(body["locatedAt"]["entity"].get("createdAt").is_some());
}

// ---------------------------------------------------------------------------
// Flat linked entity with sysAttrs
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_flat_linked_sys_attrs_includes_created_at() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:flat_sys");
    let city_id = unique_id("urn:ngsi-ld:City:flat_sys");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=flat&joinLevel=1&options=sysAttrs",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("flat join should return array");
    for entity in arr {
        assert!(entity.get("createdAt").is_some());
    }
}

// ---------------------------------------------------------------------------
// Two-level linked entities
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_inline_two_level_returns_nested() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:two_inline");
    let city_id = unique_id("urn:ngsi-ld:City:two_inline");
    let country_id = unique_id("urn:ngsi-ld:Country:two_inline");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_relationship(&city_id, &country_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=inline&joinLevel=2",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], building_id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_flat_two_level_returns_all() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:two_flat");
    let city_id = unique_id("urn:ngsi-ld:City:two_flat");
    let country_id = unique_id("urn:ngsi-ld:Country:two_flat");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_relationship(&city_id, &country_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &building_id, "join=flat&joinLevel=2", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("flat join should return array");
    assert!(arr.len() >= 3);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_inline_two_level_simplified_returns_nested() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:two_inline_kv");
    let city_id = unique_id("urn:ngsi-ld:City:two_inline_kv");
    let country_id = unique_id("urn:ngsi-ld:Country:two_inline_kv");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_relationship(&city_id, &country_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=inline&joinLevel=2&options=keyValues",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], building_id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_flat_two_level_simplified_returns_all() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:two_flat_kv");
    let city_id = unique_id("urn:ngsi-ld:City:two_flat_kv");
    let country_id = unique_id("urn:ngsi-ld:Country:two_flat_kv");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_relationship(&city_id, &country_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=flat&joinLevel=2&options=keyValues",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("flat join should return array");
    assert!(arr.len() >= 3);
}

// ---------------------------------------------------------------------------
// Non-existent linked entity
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_inline_nonexistent_linked_returns_entity() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:inline_nolink");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, "urn:ngsi-ld:City:NonExistent")).await,
        StatusCode::CREATED
    );

    let (status, body) = get_entity(
        &client,
        &building_id,
        "join=inline&joinLevel=1",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], building_id);
}

#[tokio::test]
#[serial]
async fn entity_retrieve_flat_nonexistent_linked_returns_entity() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:flat_nolink");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, "urn:ngsi-ld:City:NonExistent")).await,
        StatusCode::CREATED
    );

    let (status, _) = get_entity(&client, &building_id, "join=flat&joinLevel=1", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Format and options parameters
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_retrieve_format_key_values_returns_simplified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:format_kv");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "format=keyValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body["name"].is_string());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_format_simplified_returns_simplified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:format_simple");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "format=simplified", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body["name"].is_string());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_format_normalized_returns_normalized() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:format_norm");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "format=normalized", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert_eq!(body["name"]["type"], "Property");
}

#[tokio::test]
#[serial]
async fn entity_retrieve_options_key_values_returns_simplified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:options_kv");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "options=keyValues", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body["name"].is_string());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_options_simplified_returns_simplified() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:options_simple");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "options=simplified", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body["name"].is_string());
}

#[tokio::test]
#[serial]
async fn entity_retrieve_options_normalized_returns_normalized() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:options_norm");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&client, &id, "options=normalized", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert_eq!(body["name"]["type"], "Property");
}
