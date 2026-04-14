//! Entity query integration tests.

use crate::{
    COMPOUND_CONTEXT,
    TEST_SUITE_CONTEXT,
    batch_query,
    building_location,
    building_location_polygon,
    building_location_polygon_second,
    building_location_second,
    building_location_third,
    building_minimal,
    building_relationship,
    building_simple_attributes,
    building_simple_attributes_second,
    building_simple_attributes_third,
    building_with_different_type,
    building_with_many_scopes,
    building_with_one_scope,
    building_with_two_types,
    city_minimal,
    city_relationship,
    clean_database,
    client,
    country_minimal,
    create_entity,
    create_entity_json,
    parking_simple_attributes,
    query_entities,
    unique_id,
    vehicle_simple_attributes,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Query by IDs
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_by_ids_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:query_ids_1");
    let id2 = unique_id("urn:ngsi-ld:Building:query_ids_2");
    let id3 = unique_id("urn:ngsi-ld:Building:query_ids_3");

    for id in [&id1, &id2, &id3] {
        let status = create_entity(&client, &building_minimal(id)).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let query = format!("id={},{}&type=Building", id1, id2);
    let (status, body, _) = query_entities(&client, &query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

// ---------------------------------------------------------------------------
// Query by type
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_by_type_returns_matching_entities() {
    clean_database().await;
    let client = client();

    let building_id = unique_id("urn:ngsi-ld:Building:query_type");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:query_type");
    let parking_id = unique_id("urn:ngsi-ld:OffStreetParking:query_type");

    assert_eq!(create_entity(&client, &building_minimal(&building_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &vehicle_simple_attributes(&vehicle_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &parking_simple_attributes(&parking_id)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=Vehicle,OffStreetParking", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

// ---------------------------------------------------------------------------
// Query by id pattern
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_by_id_pattern_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let prefix = format!("urn:ngsi-ld:Building:idpat_{}", uuid::Uuid::new_v4());
    let id1 = format!("{prefix}a");
    let id2 = format!("{prefix}b");

    for id in [&id1, &id2] {
        assert_eq!(create_entity(&client, &building_minimal(id)).await, StatusCode::CREATED);
    }

    let query = format!("idPattern={prefix}.*&type=Building");
    let (status, body, _) = query_entities(&client, &query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

// ---------------------------------------------------------------------------
// Query by attrs
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_by_attrs_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:query_attrs_1");
    let id2 = unique_id("urn:ngsi-ld:Building:query_attrs_2");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_minimal(&id2)).await, StatusCode::CREATED);

    let query = "attrs=https://ngsi-ld-test-suite/context%23airQualityLevel,https://ngsi-ld-test-suite/context%23subCategory";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], id1);
}

// ---------------------------------------------------------------------------
// Query with geoquery
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_by_geoquery_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:geo_1");
    let id2 = unique_id("urn:ngsi-ld:Building:geo_2");

    assert_eq!(create_entity(&client, &building_location(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location(&id2)).await, StatusCode::CREATED);

    let query = "type=Building&georel=equals&geometry=Point&coordinates=%5B13.3986,52.5547%5D";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

// ---------------------------------------------------------------------------
// Scope queries
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_scope_full_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:scope_full_1");
    let id2 = unique_id("urn:ngsi-ld:Building:scope_full_2");

    assert_eq!(
        create_entity_json(&client, &building_with_one_scope(&id1), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity_json(&client, &building_with_many_scopes(&id2), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );

    let query = "type=Building&scopeQ=/Madrid/Gardens/ParqueNorte";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_scope_plus_wildcard_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:scope_plus_1");
    let id2 = unique_id("urn:ngsi-ld:Building:scope_plus_2");

    assert_eq!(
        create_entity_json(&client, &building_with_one_scope(&id1), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity_json(&client, &building_with_many_scopes(&id2), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );

    let query = "type=Building&scopeQ=/Madrid/%2B/ParqueNorte";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_scope_hash_wildcard_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:scope_hash_1");
    let id2 = unique_id("urn:ngsi-ld:Building:scope_hash_2");

    assert_eq!(
        create_entity_json(&client, &building_with_one_scope(&id1), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity_json(&client, &building_with_many_scopes(&id2), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );

    let query = "type=Building&scopeQ=/CompanyA/%23";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
}

#[tokio::test]
#[serial]
async fn entity_query_scope_nonempty_returns_entities_with_scope() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:scope_nonempty_1");
    let id2 = unique_id("urn:ngsi-ld:Building:scope_nonempty_2");

    assert_eq!(
        create_entity_json(&client, &building_with_one_scope(&id1), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity_json(&client, &building_with_many_scopes(&id2), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );

    let query = "type=Building&scopeQ=/%23";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_scope_and_returns_intersection() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:scope_and_1");
    let id2 = unique_id("urn:ngsi-ld:Building:scope_and_2");

    assert_eq!(
        create_entity_json(&client, &building_with_one_scope(&id1), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity_json(&client, &building_with_many_scopes(&id2), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );

    let query = "type=Building&scopeQ=/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
}

#[tokio::test]
#[serial]
async fn entity_query_scope_or_returns_union() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:scope_or_1");
    let id2 = unique_id("urn:ngsi-ld:Building:scope_or_2");

    assert_eq!(
        create_entity_json(&client, &building_with_one_scope(&id1), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity_json(&client, &building_with_many_scopes(&id2), COMPOUND_CONTEXT).await,
        StatusCode::CREATED
    );

    let query = "type=Building&scopeQ=/Madrid/Gardens/ParqueNorte,/CompanyA/OrganizationB/UnitC";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

// ---------------------------------------------------------------------------
// POST query
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_post_by_id_returns_matching_entity() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:post_id_1");
    let id2 = unique_id("urn:ngsi-ld:Building:post_id_2");
    let id3 = unique_id("urn:ngsi-ld:Building:post_id_3");

    for id in [&id1, &id2, &id3] {
        assert_eq!(create_entity(&client, &building_minimal(id)).await, StatusCode::CREATED);
    }

    let query_body = json!({
        "entities": [{"id": &id1, "type": "Building"}],
        "@context": TEST_SUITE_CONTEXT
    });
    let (status, body) = batch_query(&client, &query_body, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], id1);
}

#[tokio::test]
#[serial]
async fn entity_query_post_by_type_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:post_type");
    let vehicle_id1 = unique_id("urn:ngsi-ld:Vehicle:post_type_1");
    let vehicle_id2 = unique_id("urn:ngsi-ld:Vehicle:post_type_2");

    assert_eq!(create_entity(&client, &building_minimal(&building_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &vehicle_simple_attributes(&vehicle_id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &vehicle_simple_attributes(&vehicle_id2)).await, StatusCode::CREATED);

    let query_body = json!({
        "entities": [{"type": "Vehicle"}],
        "@context": COMPOUND_CONTEXT
    });
    let (status, body) = batch_query(&client, &query_body, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_post_by_id_pattern_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let prefix = format!("urn:ngsi-ld:Building:postpat_{}", uuid::Uuid::new_v4());
    let id1 = format!("{prefix}a");
    let id2 = format!("{prefix}b");

    for id in [&id1, &id2] {
        assert_eq!(create_entity(&client, &building_minimal(id)).await, StatusCode::CREATED);
    }

    let query_body = json!({
        "entities": [{"idPattern": format!("{prefix}.*"), "type": "Building"}],
        "@context": TEST_SUITE_CONTEXT
    });
    let (status, body) = batch_query(&client, &query_body, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_post_by_attrs_returns_matching_entities() {
    clean_database().await;
    let client = client();
    let building_id = unique_id("urn:ngsi-ld:Building:post_attrs");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:post_attrs");

    assert_eq!(create_entity(&client, &building_minimal(&building_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &vehicle_simple_attributes(&vehicle_id)).await, StatusCode::CREATED);

    let query_body = json!({
        "attrs": ["brandName", "isParked"],
        "@context": TEST_SUITE_CONTEXT
    });
    let (status, body) = batch_query(&client, &query_body, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
}

#[tokio::test]
#[serial]
async fn entity_query_post_geojson_returns_feature_collection() {
    clean_database().await;
    let client = client();
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:post_geojson");
    let parking_id = unique_id("urn:ngsi-ld:OffStreetParking:post_geojson");

    assert_eq!(create_entity(&client, &vehicle_simple_attributes(&vehicle_id)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &parking_simple_attributes(&parking_id)).await, StatusCode::CREATED);

    let query_body = json!({
        "type": "Query",
        "entities": [{"type": "Vehicle"}, {"type": "OffStreetParking"}],
        "@context": COMPOUND_CONTEXT
    });
    let (status, body) = batch_query(&client, &query_body, "application/geo+json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "FeatureCollection");
}

#[tokio::test]
#[serial]
async fn entity_query_post_two_ids_returns_both() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:post_two_1");
    let id2 = unique_id("urn:ngsi-ld:Building:post_two_2");
    let id3 = unique_id("urn:ngsi-ld:Building:post_two_3");

    for id in [&id1, &id2, &id3] {
        assert_eq!(create_entity(&client, &building_minimal(id)).await, StatusCode::CREATED);
    }

    let query_body = json!({
        "entities": [
            {"id": &id1, "type": "Building"},
            {"id": &id2, "type": "Building"}
        ],
        "@context": TEST_SUITE_CONTEXT
    });
    let (status, body) = batch_query(&client, &query_body, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

// ---------------------------------------------------------------------------
// Invalid query params
// ---------------------------------------------------------------------------

#[tokio::test]
async fn entity_query_invalid_ids_returns_bad_request() {
    let client = client();
    let (status, body, _) = query_entities(&client, "id=thisisaninvaliduri1,thisisaninvaliduri2&type=Building", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
}

#[tokio::test]
async fn entity_query_invalid_id_pattern_returns_bad_request() {
    let client = client();
    let (status, body, _) = query_entities(&client, "idPattern=invalid_entity_id_pattern**&type=Building", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
}

#[tokio::test]
async fn entity_query_invalid_attrs_returns_bad_request() {
    let client = client();
    let (status, body, _) = query_entities(&client, "attrs=id,type", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
}

#[tokio::test]
async fn entity_query_invalid_georel_returns_bad_request() {
    let client = client();
    let query = "type=Building&georel=invalid&geometry=Point&coordinates=%5B-8.503,41.202%5D&geoproperty=location";
    let (status, body, _) = query_entities(&client, query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
}

// ---------------------------------------------------------------------------
// Simplified representation
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_key_values_returns_simplified() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:kv_1");
    let id2 = unique_id("urn:ngsi-ld:Building:kv_2");

    for id in [&id1, &id2] {
        assert_eq!(create_entity(&client, &building_simple_attributes(id)).await, StatusCode::CREATED);
    }

    let query = format!("id={},{}&type=Building&options=keyValues", id1, id2);
    let (status, body, _) = query_entities(&client, &query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
    for entity in arr {
        assert!(entity["name"].is_string(), "name should be simplified");
    }
}

// ---------------------------------------------------------------------------
// GeoJSON query
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_geojson_returns_feature_collection() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:geojson_1");
    let id2 = unique_id("urn:ngsi-ld:Building:geojson_2");

    assert_eq!(create_entity(&client, &building_location(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location(&id2)).await, StatusCode::CREATED);

    let query = format!("id={},{}&type=Building", id1, id2);
    let (status, body, _) = query_entities(&client, &query, "application/geo+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "FeatureCollection");
    assert!(body["features"].as_array().is_some());
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_with_limit_returns_limited_results() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:limit_1");
    let id2 = unique_id("urn:ngsi-ld:Building:limit_2");
    let id3 = unique_id("urn:ngsi-ld:Building:limit_3");

    for id in [&id1, &id2, &id3] {
        assert_eq!(create_entity(&client, &building_minimal(id)).await, StatusCode::CREATED);
    }

    let query = format!("id={},{},{}&type=Building&limit=2", id1, id2, id3);
    let (status, body, _) = query_entities(&client, &query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

// ---------------------------------------------------------------------------
// Count header
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_count_returns_header() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:count_1");
    let id2 = unique_id("urn:ngsi-ld:Building:count_2");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_simple_attributes_second(&id2)).await, StatusCode::CREATED);

    let (status, _, count) = query_entities(&client, "type=Building&count=true", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("2".to_owned()));
}

#[tokio::test]
#[serial]
async fn entity_query_count_with_q_returns_filtered_count() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:count_q_1");
    let id2 = unique_id("urn:ngsi-ld:Building:count_q_2");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_simple_attributes_second(&id2)).await, StatusCode::CREATED);

    let (status, _, count) = query_entities(
        &client,
        "type=Building&q=airQualityLevel==6&count=true",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("1".to_owned()));
}

// ---------------------------------------------------------------------------
// Entity Type Selection Language
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_type_selection_one_type() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:MultiTypes:type_sel_1");
    let id2 = unique_id("urn:ngsi-ld:MultiTypes:type_sel_2");
    let id3 = unique_id("urn:ngsi-ld:MultiTypes:type_sel_3");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_with_different_type(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_with_two_types(&id3)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=Building", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_type_selection_and_operator() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:MultiTypes:type_and_1");
    let id2 = unique_id("urn:ngsi-ld:MultiTypes:type_and_2");
    let id3 = unique_id("urn:ngsi-ld:MultiTypes:type_and_3");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_with_different_type(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_with_two_types(&id3)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=(Building;TouristDestination)", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
}

#[tokio::test]
#[serial]
async fn entity_query_type_selection_or_operator() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:MultiTypes:type_or_1");
    let id2 = unique_id("urn:ngsi-ld:MultiTypes:type_or_2");
    let id3 = unique_id("urn:ngsi-ld:MultiTypes:type_or_3");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_with_different_type(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_with_two_types(&id3)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=Building,Parking", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 3);
}

// ---------------------------------------------------------------------------
// Query with q parameter
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_q_name_equals_returns_matching() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:q_name_1");
    let id2 = unique_id("urn:ngsi-ld:Building:q_name_2");
    let id3 = unique_id("urn:ngsi-ld:Building:q_name_3");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_simple_attributes_second(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_simple_attributes_third(&id3)).await, StatusCode::CREATED);

    let (status, _, count) = query_entities(
        &client,
        "q=name==%22Eiffel Tower%22&count=true",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("1".to_owned()));
}

#[tokio::test]
#[serial]
async fn entity_query_q_relationship_returns_matching() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:q_rel_1");
    let id2 = unique_id("urn:ngsi-ld:Building:q_rel_2");
    let id3 = unique_id("urn:ngsi-ld:Building:q_rel_3");

    assert_eq!(create_entity(&client, &building_simple_attributes(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_simple_attributes_second(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_simple_attributes_third(&id3)).await, StatusCode::CREATED);

    let (status, _, count) = query_entities(
        &client,
        "q=locatedIn==urn:ngsi-ld:City:Pisa&count=true",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("1".to_owned()));
}

// ---------------------------------------------------------------------------
// Geoquery with Point
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_geo_equals_point_returns_matching() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:geo_eq_1");
    let id2 = unique_id("urn:ngsi-ld:Building:geo_eq_2");
    let id3 = unique_id("urn:ngsi-ld:Building:geo_eq_3");

    assert_eq!(create_entity(&client, &building_location(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location_second(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location_third(&id3)).await, StatusCode::CREATED);

    let query = "type=Building&georel=equals&geometry=Point&coordinates=%5B13.3986,52.5547%5D&count=true";
    let (status, _, count) = query_entities(&client, query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("1".to_owned()));
}

#[tokio::test]
#[serial]
async fn entity_query_geo_near_max_distance_returns_matching() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:geo_near_1");
    let id2 = unique_id("urn:ngsi-ld:Building:geo_near_2");
    let id3 = unique_id("urn:ngsi-ld:Building:geo_near_3");

    assert_eq!(create_entity(&client, &building_location(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location_second(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location_third(&id3)).await, StatusCode::CREATED);

    let query = "type=Building&georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D&count=true";
    let (status, _, count) = query_entities(&client, query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("3".to_owned()));
}

#[tokio::test]
#[serial]
async fn entity_query_geo_within_polygon_returns_matching() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:geo_within_1");
    let id2 = unique_id("urn:ngsi-ld:Building:geo_within_2");
    let id3 = unique_id("urn:ngsi-ld:Building:geo_within_3");

    assert_eq!(create_entity(&client, &building_location(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location_second(&id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location_third(&id3)).await, StatusCode::CREATED);

    let coords =
        "%5B%5B%5B13.1616211,52.5438124%5D,%5B13.597641,52.4840224%5D,%5B13.5948944,52.3402782%5D,%5B13.142395,52.386428%5D,%5B13.1616211,52.5438124%5D%5D%5D";
    let query = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}&count=true");
    let (status, _, count) = query_entities(&client, &query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("1".to_owned()));
}

// ---------------------------------------------------------------------------
// Geoquery with Polygon
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_geo_polygon_equals_returns_matching() {
    clean_database().await;
    let client = client();
    let id1 = unique_id("urn:ngsi-ld:Building:geo_poly_1");
    let id2 = unique_id("urn:ngsi-ld:Building:geo_poly_2");

    assert_eq!(create_entity(&client, &building_location_polygon(&id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &building_location_polygon_second(&id2)).await, StatusCode::CREATED);

    let coords = "%5B%5B%5B13.2865906,52.5648645%5D,%5B13.2879639,52.5648645%5D,%5B13.2797241,52.4988679%5D,%5B13.477478,52.4712703%5D,%5B13.5049438,52.5373084%5D,%5B13.2865906,52.5648645%5D%5D%5D";
    let query = format!("type=Building&georel=equals&geometry=Polygon&coordinates={coords}&count=true");
    let (status, _, count) = query_entities(&client, &query, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count, Some("1".to_owned()));
}

// ---------------------------------------------------------------------------
// Dataset ID filter
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_dataset_attrs_only_returns_entity() {
    clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:dataset_attrs");

    assert_eq!(create_entity(&client, &building_multi_instances(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=Building&attrs=name", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
}

fn building_multi_instances(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "type": "Building",
        "name": [
            {
                "type": "Property",
                "value": "Eiffel Tower"
            },
            {
                "type": "Property",
                "value": "Tour Eiffel",
                "datasetId": "urn:ngsi-ld:Dataset:french-name"
            },
            {
                "type": "Property",
                "value": "Eiffelturm",
                "datasetId": "urn:ngsi-ld:Dataset:german-name"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

#[tokio::test]
#[serial]
async fn entity_query_dataset_id_only_returns_entity() {
    clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:dataset_id");

    assert_eq!(create_entity(&client, &building_multi_instances(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(
        &client,
        "type=Building&datasetId=urn:ngsi-ld:Dataset:french-name",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
}

#[tokio::test]
#[serial]
async fn entity_query_dataset_default_instance_returns_entity() {
    clean_database().await;
    let client = client();
    let id = unique_id("urn:ngsi-ld:Building:dataset_default");

    assert_eq!(create_entity(&client, &building_multi_instances(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=Building&datasetId=@none", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 1);
}

// ---------------------------------------------------------------------------
// One-level linked entities in query
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_inline_linked_returns_nested_entities() {
    clean_database().await;
    let client = client();

    let building_id1 = unique_id("urn:ngsi-ld:Building:linked_1");
    let building_id2 = unique_id("urn:ngsi-ld:Building:linked_2");
    let city_id1 = unique_id("urn:ngsi-ld:City:linked_1");
    let city_id2 = unique_id("urn:ngsi-ld:City:linked_2");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id1, &city_id1)).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity(&client, &building_relationship(&building_id2, &city_id2)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &city_minimal(&city_id2)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(
        &client,
        "type=Building&join=inline&joinLevel=1",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_flat_linked_returns_flattened_entities() {
    clean_database().await;
    let client = client();

    let building_id1 = unique_id("urn:ngsi-ld:Building:flat_1");
    let building_id2 = unique_id("urn:ngsi-ld:Building:flat_2");
    let city_id1 = unique_id("urn:ngsi-ld:City:flat_1");
    let city_id2 = unique_id("urn:ngsi-ld:City:flat_2");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id1, &city_id1)).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity(&client, &building_relationship(&building_id2, &city_id2)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &city_minimal(&city_id2)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=Building&join=flat&joinLevel=1", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert!(arr.len() >= 4, "should contain 2 buildings + 2 cities");
}

// ---------------------------------------------------------------------------
// Two-level linked entities in query
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_inline_two_level_returns_nested() {
    clean_database().await;
    let client = client();

    let building_id1 = unique_id("urn:ngsi-ld:Building:twolevel_1");
    let building_id2 = unique_id("urn:ngsi-ld:Building:twolevel_2");
    let city_id1 = unique_id("urn:ngsi-ld:City:twolevel_1");
    let city_id2 = unique_id("urn:ngsi-ld:City:twolevel_2");
    let country_id1 = unique_id("urn:ngsi-ld:Country:twolevel_1");
    let country_id2 = unique_id("urn:ngsi-ld:Country:twolevel_2");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id1, &city_id1)).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity(&client, &building_relationship(&building_id2, &city_id2)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_relationship(&city_id1, &country_id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &city_relationship(&city_id2, &country_id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id2)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(
        &client,
        "type=Building&join=inline&joinLevel=2",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
#[serial]
async fn entity_query_flat_two_level_returns_all_entities() {
    clean_database().await;
    let client = client();

    let building_id1 = unique_id("urn:ngsi-ld:Building:twolevel_flat_1");
    let building_id2 = unique_id("urn:ngsi-ld:Building:twolevel_flat_2");
    let city_id1 = unique_id("urn:ngsi-ld:City:twolevel_flat_1");
    let city_id2 = unique_id("urn:ngsi-ld:City:twolevel_flat_2");
    let country_id1 = unique_id("urn:ngsi-ld:Country:twolevel_flat_1");
    let country_id2 = unique_id("urn:ngsi-ld:Country:twolevel_flat_2");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id1, &city_id1)).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_entity(&client, &building_relationship(&building_id2, &city_id2)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_relationship(&city_id1, &country_id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &city_relationship(&city_id2, &country_id2)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id1)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&client, &country_minimal(&country_id2)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(&client, "type=Building&join=flat&joinLevel=2", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    assert!(arr.len() >= 6, "should contain buildings, cities, and countries");
}

// ---------------------------------------------------------------------------
// Linked entities + sysAttrs
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_query_inline_linked_sys_attrs_returns_created_at() {
    clean_database().await;
    let client = client();

    let building_id = unique_id("urn:ngsi-ld:Building:linked_sys_1");
    let city_id = unique_id("urn:ngsi-ld:City:linked_sys_1");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(
        &client,
        "type=Building&join=inline&joinLevel=1&options=sysAttrs",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    for entity in arr {
        assert!(entity["locatedAt"]["entity"].get("createdAt").is_some(), "linked entity should have createdAt");
    }
}

#[tokio::test]
#[serial]
async fn entity_query_flat_linked_sys_attrs_returns_created_at() {
    clean_database().await;
    let client = client();

    let building_id = unique_id("urn:ngsi-ld:Building:flat_sys_1");
    let city_id = unique_id("urn:ngsi-ld:City:flat_sys_1");

    assert_eq!(
        create_entity(&client, &building_relationship(&building_id, &city_id)).await,
        StatusCode::CREATED
    );
    assert_eq!(create_entity(&client, &city_minimal(&city_id)).await, StatusCode::CREATED);

    let (status, body, _) = query_entities(
        &client,
        "type=Building&join=flat&joinLevel=1&options=sysAttrs",
        "application/ld+json",
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("should return array");
    for entity in arr {
        assert!(entity.get("createdAt").is_some(), "all entities should have createdAt");
    }
}
