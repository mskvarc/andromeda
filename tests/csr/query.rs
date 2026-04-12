//! CSR query tests (NGSI-LD 5.10.2, 5.5.9)

use reqwest::StatusCode;
use serial_test::serial;

use crate::{
    BAD_REQUEST_DATA_TYPE, clean_database, client, create_registration, load_expectation, load_fixture, query_registrations,
    strip_ignored_keys, unique_id,
};
use serde_json::Value;

const TEST_SUITE_CONTEXT_URL: &str =
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";

/// Query helper with Link header context for test suite context.
async fn query_with_context(client: &reqwest::Client, qs: &str) -> (StatusCode, Value) {
    query_registrations(client, qs, Some("application/ld+json"), Some(TEST_SUITE_CONTEXT_URL)).await
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with entity types or attribute names
// ---------------------------------------------------------------------------

/// Query by type=Building -- returns matching registrations (5.10.2).
#[tokio::test]
#[serial]
async fn query_by_entity_type_returns_matching() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-type-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-type-b");

    let reg1 = load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
    let reg2 = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);

    let (s, _, _) = create_registration(&c, &reg1).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Building").await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-01.json");
    expected[0]["id"] = Value::String(id2.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

/// Query by attrs=name -- returns matching registrations (5.10.2).
#[tokio::test]
#[serial]
async fn query_by_attribute_name_returns_matching() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-attr-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-attr-b");

    let reg1 = load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
    let reg2 = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);

    let (s, _, _) = create_registration(&c, &reg1).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "attrs=name").await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-01.json");
    expected[0]["id"] = Value::String(id2.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query without entity types or attribute names
// ---------------------------------------------------------------------------

/// Query with no type or attrs -- 400 (5.10.2).
#[tokio::test]
#[serial]
async fn query_without_type_or_attrs_returns_400() {
    clean_database().await;
    let c = client();
    let (status, body) = query_with_context(&c, "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with invalid query parameters
// ---------------------------------------------------------------------------

/// Query with invalid id parameter -- 400 (5.10.2).
#[tokio::test]
#[serial]
async fn query_invalid_id_returns_400() {
    clean_database().await;
    let c = client();
    let (status, body) = query_with_context(&c, "id=invalidUri").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

/// Query with invalid q expression -- 400 (5.10.2).
#[tokio::test]
#[serial]
async fn query_invalid_q_returns_400() {
    clean_database().await;
    let c = client();
    let (status, body) = query_with_context(&c, "q=(invalidQuery").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

/// Query with invalid georel (missing geometry/coordinates) -- 400 (5.10.2).
#[tokio::test]
#[serial]
async fn query_invalid_geoquery_returns_400() {
    clean_database().await;
    let c = client();
    let (status, body) = query_with_context(&c, "georel=within").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

/// Query with invalid timerel (missing timeAt) -- 400 (5.10.2).
#[tokio::test]
#[serial]
async fn query_invalid_temporal_query_returns_400() {
    clean_database().await;
    let c = client();
    let (status, body) = query_with_context(&c, "timerel=before").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query without context (fully qualified type)
// ---------------------------------------------------------------------------

/// Query with FQ type URI, no Link context -- 200 (5.10.2).
#[tokio::test]
#[serial]
async fn query_fully_qualified_type_without_context_returns_200() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-fq");
    let reg = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "id={id}&type={}",
        urlencoding::encode("https://ngsi-ld-test-suite/context#Building")
    );
    let (status, mut actual) = query_registrations(&c, &qs, None, None).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-04.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query matching EntityInfo of RegistrationInfo
// ---------------------------------------------------------------------------

/// Query with type+idPattern matching EntityInfo -- 200 (5.10.2).
#[tokio::test]
#[serial]
async fn query_entity_info_with_type_and_id_pattern_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-entity-info");
    let reg = load_fixture("csource_registrations/context-source-registration.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=OffStreetParking&idPattern={}",
        urlencoding::encode(".*downtown$")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-05-01.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

/// Registration without EntityInfo matched by type+idPattern -- 200 (5.10.2).
#[tokio::test]
#[serial]
async fn query_registration_without_entity_info_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-no-entity-info");
    let reg = load_fixture(
        "csource_registrations/context-source-registration-with-only-properties-information.jsonld",
        &id,
    );

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=OffStreetParking&idPattern={}",
        urlencoding::encode(".*downtown$")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-05-02.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query matching properties/relationships
// ---------------------------------------------------------------------------

/// Query with type+attrs -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_by_type_and_attrs_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-attrs");
    let reg = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Building&attrs=name,locatedAt").await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-06.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

/// Query with type only (no attrs) -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_by_type_without_attrs_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-type-only");
    let reg = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Building").await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-06.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query matching geoquery
// ---------------------------------------------------------------------------

/// Geoquery near point -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_geoquery_near_point_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-geo-near");
    let reg = load_fixture("csource_registrations/context-source-registration-location.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&georel={}&geometry=Point&coordinates={}",
        urlencoding::encode("near;maxDistance==2000"),
        urlencoding::encode("[-8.503,41.202]")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-07.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

/// Geoquery within polygon -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_geoquery_within_polygon_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-geo-polygon");
    let reg = load_fixture("csource_registrations/context-source-registration-location.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&georel=within&geometry=Polygon&coordinates={}&geoproperty=location",
        urlencoding::encode("[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-15.23,21.37]]")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-07.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query without temporal query
// ---------------------------------------------------------------------------

/// Query with type=Vehicle, no temporal filter -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_without_temporal_query_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-no-temporal");
    let reg = load_fixture("csource_registrations/context-source-registration.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Vehicle").await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-08.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with temporal queries
// ---------------------------------------------------------------------------

/// observationInterval with observedAt timeproperty -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_observation_interval_with_observed_at_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-obs-at");
    let reg = load_fixture("csource_registrations/context-source-registration-observationInterval.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&timeproperty=observedAt&timerel=before&timeAt={}",
        urlencoding::encode("2021-08-01T22:00:00Z")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-09-01.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

/// observationInterval without explicit timeproperty (defaults to observedAt) -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_observation_interval_without_timeproperty_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-obs-default");
    let reg = load_fixture("csource_registrations/context-source-registration-observationInterval.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&timerel=before&timeAt={}",
        urlencoding::encode("2021-08-01T22:00:00Z")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-09-01.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

/// managementInterval with createdAt timeproperty -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_management_interval_with_created_at_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-mgmt-created");
    let reg = load_fixture("csource_registrations/context-source-registration-managementInterval.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&timeproperty=createdAt&timerel=before&timeAt={}",
        urlencoding::encode("2021-08-01T22:00:00Z")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-09-02.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

/// managementInterval with modifiedAt timeproperty -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_management_interval_with_modified_at_returns_matching() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-mgmt-modified");
    let reg = load_fixture("csource_registrations/context-source-registration-managementInterval.jsonld", &id);

    let (s, _, _) = create_registration(&c, &reg).await;
    assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&timeproperty=modifiedAt&timerel=before&timeAt={}",
        urlencoding::encode("2021-08-01T22:00:00Z")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-09-02.json");
    expected[0]["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with entity IDs
// ---------------------------------------------------------------------------

/// Query filtering by multiple registration IDs -- returns matching subset (5.10.2).
#[tokio::test]
#[serial]
async fn query_by_entity_ids_returns_matching_subset() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-ids-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-ids-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-ids-c");

    let reg1 = load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
    let reg2 = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);
    let reg3 = load_fixture("csource_registrations/context-source-registration-csourceProperty.jsonld", &id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, actual) = query_with_context(&c, &format!("id={id1},{id3}")).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let arr = actual.as_array().expect("response should be an array");
    assert_eq!(arr.len(), 2, "should return 2 registrations");

    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-10-01.json");
    expected[0]["id"] = Value::String(id1.clone());
    expected[1]["id"] = Value::String(id3.clone());

    let mut actual_arr: Vec<Value> = arr.clone();
    actual_arr.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));
    let mut expected_arr: Vec<Value> = expected.as_array().unwrap().clone();
    expected_arr.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));

    for v in &mut actual_arr {
        strip_ignored_keys(v, ignored);
    }
    for v in &mut expected_arr {
        strip_ignored_keys(v, ignored);
    }
    assert_eq!(expected_arr, actual_arr);
}

/// Query with q condition filtering by csourceProperty -- returns matching (5.10.2).
#[tokio::test]
#[serial]
async fn query_by_q_condition_returns_matching() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-q-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-q-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-q-c");

    let reg1 = load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
    let reg2 = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);
    let reg3 = load_fixture("csource_registrations/context-source-registration-csourceProperty.jsonld", &id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "q=csourceProperty1==\"aValue\"").await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registrations-037-10-02.json");
    expected[0]["id"] = Value::String(id3.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);
}

// ---------------------------------------------------------------------------
// 5.10.2, 5.5.9 -- Query with pagination
// ---------------------------------------------------------------------------

/// Pagination: limit=1, offset=1 -- returns second result (5.10.2, 5.5.9).
#[tokio::test]
#[serial]
async fn query_pagination_limit_1_offset_1_returns_one_result() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-c");

    let reg1 = load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
    let reg2 = load_fixture("csource_registrations/context-source-registration-location.jsonld", &id2);
    let reg3 = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = query_with_context(&c, "type=Building&limit=1&offset=1").await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("response should be an array");
    assert_eq!(arr.len(), 1, "should return 1 registration");
}

/// Pagination: limit=1, offset=0 -- returns first result (5.10.2, 5.5.9).
#[tokio::test]
#[serial]
async fn query_pagination_limit_1_offset_0_returns_one_result() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-first-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-first-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-first-c");

    let reg1 = load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
    let reg2 = load_fixture("csource_registrations/context-source-registration-location.jsonld", &id2);
    let reg3 = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = query_with_context(&c, "type=Building&limit=1&offset=0").await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("response should be an array");
    assert_eq!(arr.len(), 1, "should return 1 registration");
}

/// Pagination: limit=15, offset=0 -- returns all matching (only Building types) (5.10.2, 5.5.9).
#[tokio::test]
#[serial]
async fn query_pagination_returns_all_matching_within_limit() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-all-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-all-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-all-c");

    let reg1 = load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
    let reg2 = load_fixture("csource_registrations/context-source-registration-location.jsonld", &id2);
    let reg3 = load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = query_with_context(&c, "type=Building&limit=15&offset=0").await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("response should be an array");
    assert_eq!(arr.len(), 2, "should return 2 registrations (only Building type matches)");
}
