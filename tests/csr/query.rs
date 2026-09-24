#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! CSR query tests (NGSI-LD 5.10.2, 5.5.9)

use reqwest::StatusCode;

use crate::{
    BAD_REQUEST_DATA_TYPE,
    REGISTERED_VEHICLE_ID,
    UNREGISTERED_VEHICLE_ID,
    create_registration,
    csr_basic_jsonld,
    csr_detailed_information,
    csr_only_properties,
    csr_vehicle_by_entity_id,
    csr_vehicle_by_type,
    csr_with_csource_properties,
    csr_with_location,
    csr_with_management_interval,
    csr_with_observation_interval,
    expected_query_detailed,
    expected_query_detailed_expanded,
    expected_query_matching_entity_ids,
    expected_query_offstreetparking_information,
    expected_query_only_properties,
    expected_query_vehicle_information,
    expected_query_with_csource_properties,
    expected_query_with_location,
    expected_query_with_management_interval,
    expected_query_with_observation_interval,
    query_registrations,
    strip_ignored_keys,
    unique_id,
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use serde_json::Value;

const TEST_SUITE_CONTEXT_URL: &str =
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";

/// The `timeAt` a `before` or `after` discovery has to carry to match the
/// interval the interval fixtures register (2020-08-01T22:07:00Z to
/// 2021-08-01T21:07:00Z).
///
/// Clause 5.10.2.4: "The semantics of the match is that the timeAt in the case
/// of the 'before' and 'after' relation is contained in or is an endpoint of a
/// time period included in the specified time interval." An instant past the
/// registered `endAt` is therefore not a match, however far "before" it reads.
const WITHIN_REGISTERED_INTERVAL: &str = "2021-08-01T20:00:00Z";

/// Query helper with Link header context for test suite context.
async fn query_with_context(client: &TestClient, qs: &str) -> (StatusCode, Value) {
    query_registrations(client, qs, Some("application/ld+json"), Some(TEST_SUITE_CONTEXT_URL)).await
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with entity types or attribute names
// ---------------------------------------------------------------------------

/// Query by type=Building -- returns matching registrations (5.10.2).
#[tokio::test]
async fn query_by_entity_type_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-type-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-type-b");

    let reg1 = csr_basic_jsonld(&id1);
    let reg2 = csr_detailed_information(&id2);

    let (s, _, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Building").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_detailed(&id2);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

/// Query by attrs=name -- returns matching registrations (5.10.2).
#[tokio::test]
async fn query_by_attribute_name_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-attr-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-attr-b");

    let reg1 = csr_basic_jsonld(&id1);
    let reg2 = csr_detailed_information(&id2);

    let (s, _, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "attrs=name").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_detailed(&id2);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query without entity types or attribute names
// ---------------------------------------------------------------------------

/// Query with no type or attrs -- 400 (5.10.2).
#[tokio::test]
async fn query_without_type_or_attrs_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = query_with_context(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with invalid query parameters
// ---------------------------------------------------------------------------

/// Query with invalid id parameter -- 400 (5.10.2).
#[tokio::test]
async fn query_invalid_id_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = query_with_context(&c, "id=invalidUri").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// Query with invalid q expression -- 400 (5.10.2).
#[tokio::test]
async fn query_invalid_q_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = query_with_context(&c, "q=(invalidQuery").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// Query with invalid georel (missing geometry/coordinates) -- 400 (5.10.2).
#[tokio::test]
async fn query_invalid_geoquery_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = query_with_context(&c, "georel=within").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// Query with invalid timerel (missing timeAt) -- 400 (5.10.2).
#[tokio::test]
async fn query_invalid_temporal_query_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = query_with_context(&c, "timerel=before").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query without context (fully qualified type)
// ---------------------------------------------------------------------------

/// Query with FQ type URI, no Link context -- 200 (5.10.2).
#[tokio::test]
async fn query_fully_qualified_type_without_context_returns_200() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-fq");
    let reg = csr_detailed_information(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!("id={id}&type={}", urlencoding::encode("https://ngsi-ld-test-suite/context#Building"));
    let (status, mut actual) = query_registrations(&c, &qs, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_detailed_expanded(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query matching EntityInfo of RegistrationInfo
// ---------------------------------------------------------------------------

/// Query with type+idPattern matching `EntityInfo` -- 200, trimmed to the
/// matching `RegistrationInfo` (5.10.2, 5.10.2.5).
#[tokio::test]
async fn query_entity_info_with_type_and_id_pattern_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-entity-info");
    let reg = csr_basic_jsonld(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!("type=OffStreetParking&idPattern={}", urlencoding::encode(".*downtown$"));
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_offstreetparking_information(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

/// Registration without `EntityInfo` matched by type+idPattern -- 200 (5.10.2).
#[tokio::test]
async fn query_registration_without_entity_info_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-no-entity-info");
    let reg = csr_only_properties(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!("type=OffStreetParking&idPattern={}", urlencoding::encode(".*downtown$"));
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_only_properties(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query matching properties/relationships
// ---------------------------------------------------------------------------

/// Query with type+attrs -- returns matching (5.10.2).
#[tokio::test]
async fn query_by_type_and_attrs_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-attrs");
    let reg = csr_detailed_information(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Building&attrs=name,locatedAt").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_detailed(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

/// Query with type only (no attrs) -- returns matching (5.10.2).
#[tokio::test]
async fn query_by_type_without_attrs_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-type-only");
    let reg = csr_detailed_information(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Building").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_detailed(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query matching geoquery
// ---------------------------------------------------------------------------

/// Geoquery near point -- returns matching (5.10.2).
#[tokio::test]
async fn query_geoquery_near_point_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-geo-near");
    let reg = csr_with_location(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&georel={}&geometry=Point&coordinates={}",
        urlencoding::encode("near;maxDistance==2000"),
        urlencoding::encode("[-8.503,41.202]")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_with_location(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

/// Geoquery within polygon -- returns matching (5.10.2).
#[tokio::test]
async fn query_geoquery_within_polygon_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-geo-polygon");
    let reg = csr_with_location(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&georel=within&geometry=Polygon&coordinates={}&geoproperty=location",
        urlencoding::encode("[[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-15.23,21.37],[-13.503,47.202]]]")
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_with_location(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query without temporal query
// ---------------------------------------------------------------------------

/// Query with type=Vehicle, no temporal filter -- returns matching (5.10.2).
#[tokio::test]
async fn query_without_temporal_query_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-no-temporal");
    let reg = csr_basic_jsonld(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_vehicle_information(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with temporal queries
// ---------------------------------------------------------------------------

/// observationInterval with observedAt timeproperty -- returns matching (5.10.2).
#[tokio::test]
async fn query_observation_interval_with_observed_at_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-obs-at");
    let reg = csr_with_observation_interval(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&timeproperty=observedAt&timerel=before&timeAt={}",
        urlencoding::encode(WITHIN_REGISTERED_INTERVAL)
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_with_observation_interval(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

/// observationInterval without explicit timeproperty (defaults to observedAt) -- returns matching (5.10.2).
#[tokio::test]
async fn query_observation_interval_without_timeproperty_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-obs-default");
    let reg = csr_with_observation_interval(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!("type=Building&timerel=before&timeAt={}", urlencoding::encode(WITHIN_REGISTERED_INTERVAL));
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_with_observation_interval(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

/// managementInterval with createdAt timeproperty -- returns matching (5.10.2).
#[tokio::test]
async fn query_management_interval_with_created_at_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-mgmt-created");
    let reg = csr_with_management_interval(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&timeproperty=createdAt&timerel=before&timeAt={}",
        urlencoding::encode(WITHIN_REGISTERED_INTERVAL)
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_with_management_interval(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

/// managementInterval with modifiedAt timeproperty -- returns matching (5.10.2).
#[tokio::test]
async fn query_management_interval_with_modified_at_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-mgmt-modified");
    let reg = csr_with_management_interval(&id);

    let (s, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let qs = format!(
        "type=Building&timeproperty=modifiedAt&timerel=before&timeAt={}",
        urlencoding::encode(WITHIN_REGISTERED_INTERVAL)
    );
    let (status, mut actual) = query_with_context(&c, &qs).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_with_management_interval(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2 -- Query with entity IDs
// ---------------------------------------------------------------------------

/// Query filtering by entity id -- returns the registrations that can serve
/// that entity, not the others (5.10.2, 5.12).
///
/// `id` carries "List of entity ids to be retrieved" (Table 6.8.3.2-1), and
/// clause 5.10.2.3 forbids retrieving registrations "by only specifying desired
/// Entity identifiers, without further specifying restrictions on the entities'
/// types or attributes", so the entity id is paired with a type selector.
#[tokio::test]
async fn query_by_entity_ids_returns_matching_subset() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-ids-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-ids-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-ids-c");

    let reg1 = csr_vehicle_by_entity_id(&id1, REGISTERED_VEHICLE_ID);
    let reg2 = csr_vehicle_by_entity_id(&id2, UNREGISTERED_VEHICLE_ID);
    let reg3 = csr_vehicle_by_type(&id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, actual) = query_with_context(&c, &format!("type=Vehicle&id={REGISTERED_VEHICLE_ID}")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let arr = actual.as_array().expect("response should be an array");
    pretty_assertions::assert_eq!(arr.len(), 2, "should return 2 registrations");

    let expected = expected_query_matching_entity_ids(&id1, &id3);

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
    pretty_assertions::assert_eq!(expected_arr, actual_arr);
    scope.finish().await.expect("cleanup failed");
}

/// Query with q condition filtering by csourceProperty -- returns matching (5.10.2).
#[tokio::test]
async fn query_by_q_condition_returns_matching() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-q-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-q-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-q-c");

    let reg1 = csr_basic_jsonld(&id1);
    let reg2 = csr_detailed_information(&id2);
    let reg3 = csr_with_csource_properties(&id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, mut actual) = query_with_context(&c, "q=csourceProperty1==\"aValue\"").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_query_with_csource_properties(&id3);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.10.2, 5.5.9 -- Query with pagination
// ---------------------------------------------------------------------------

/// Pagination: limit=1, offset=1 -- returns second result (5.10.2, 5.5.9).
#[tokio::test]
async fn query_pagination_limit_1_offset_1_returns_one_result() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-c");

    let reg1 = csr_basic_jsonld(&id1);
    let reg2 = csr_with_location(&id2);
    let reg3 = csr_detailed_information(&id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = query_with_context(&c, "type=Building&limit=1&offset=1").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("response should be an array");
    pretty_assertions::assert_eq!(arr.len(), 1, "should return 1 registration");
    scope.finish().await.expect("cleanup failed");
}

/// Pagination: limit=1, offset=0 -- returns first result (5.10.2, 5.5.9).
#[tokio::test]
async fn query_pagination_limit_1_offset_0_returns_one_result() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-first-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-first-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-first-c");

    let reg1 = csr_basic_jsonld(&id1);
    let reg2 = csr_with_location(&id2);
    let reg3 = csr_detailed_information(&id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = query_with_context(&c, "type=Building&limit=1&offset=0").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("response should be an array");
    pretty_assertions::assert_eq!(arr.len(), 1, "should return 1 registration");
    scope.finish().await.expect("cleanup failed");
}

/// Pagination: limit=15, offset=0 -- returns all matching (only Building types) (5.10.2, 5.5.9).
#[tokio::test]
async fn query_pagination_returns_all_matching_within_limit() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id1 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-all-a");
    let id2 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-all-b");
    let id3 = unique_id("urn:ngsi-ld:ContextSourceRegistration:query-page-all-c");

    let reg1 = csr_basic_jsonld(&id1);
    let reg2 = csr_with_location(&id2);
    let reg3 = csr_detailed_information(&id3);

    let (s, _, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = create_registration(&c, &reg3).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = query_with_context(&c, "type=Building&limit=15&offset=0").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("response should be an array");
    pretty_assertions::assert_eq!(arr.len(), 2, "should return 2 registrations (only Building type matches)");
    scope.finish().await.expect("cleanup failed");
}
