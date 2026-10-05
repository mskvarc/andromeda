#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! CSR update tests (NGSI-LD 5.9.3)

use crate::{
    BAD_REQUEST_DATA_TYPE,
    COMPOUND_CONTEXT_URL,
    LD_CONTEXT_NOT_AVAILABLE_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    UNREACHABLE_CONTEXT,
    cleanup_registration,
    create_registration,
    csr_basic_jsonld,
    csr_invalid_json_string,
    csr_invalid_null_information,
    csr_with_expiration,
    fragment_different_type,
    fragment_endpoint_update,
    fragment_null_expires_at,
    request,
    retrieve_registration,
    unique_id,
    update_registration,
    update_registration_raw,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use reqwest::{Method, StatusCode};
use serde_json::json;

// ---------------------------------------------------------------------------
// 5.9.3 -- Update endpoint
// ---------------------------------------------------------------------------

/// PATCH endpoint field on existing CSR -- 204 (5.9.3).
#[tokio::test]
async fn update_endpoint_returns_204() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-endpoint");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let fragment = fragment_endpoint_update();
    let (status, _) = update_registration(&c, &id, &fragment, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    // Clause 5.2.2 serves a Context Producer's members back as they were
    // supplied, and Table 5.2.9-1 asks only that `endpoint` be a dereferenceable
    // URI -- so the authority-only spelling the fragment carried is what the
    // retrieval answers, not the empty path a URL parser would append to it.
    pretty_assertions::assert_eq!(body["endpoint"], "http://my.new.csource.org:1026");

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update error cases
// ---------------------------------------------------------------------------

/// PATCH with invalid URI -- 400 (5.9.3).
#[tokio::test]
async fn update_invalid_uri_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let fragment = fragment_different_type("invalidURI");

    let (status, body) = request(
        &c,
        reqwest::Method::PATCH,
        "/csourceRegistrations/invalidURI",
        Some(&fragment),
        Some("application/ld+json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// PATCH with body of different data type than existing registration -- 400 (5.9.3).
#[tokio::test]
async fn update_wrong_data_type_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-wrong-type");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let fragment = fragment_different_type(&id);
    let (status, body) = request(
        &c,
        reqwest::Method::PATCH,
        &format!("/csourceRegistrations/{id}"),
        Some(&fragment),
        Some("application/ld+json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// PATCH attempting to remove mandatory property (information=null) -- 400 (5.9.3).
#[tokio::test]
async fn update_remove_mandatory_property_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-remove-mand");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let fragment = csr_invalid_null_information(&id);
    let (status, body) = request(
        &c,
        reqwest::Method::PATCH,
        &format!("/csourceRegistrations/{id}"),
        Some(&fragment),
        Some("application/ld+json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// PATCH with malformed JSON body -- 400 (5.9.3).
#[tokio::test]
async fn update_malformed_json_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-malformed");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let raw = csr_invalid_json_string();
    let (status, body) = update_registration_raw(&c, &id, raw.as_bytes(), "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
    assert!(body["title"].is_string(), "response should contain title");

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update unknown registration
// ---------------------------------------------------------------------------

/// PATCH unknown registration -- 404 (5.9.3).
#[tokio::test]
async fn update_unknown_registration_returns_404() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-unknown");
    let fragment = csr_basic_jsonld(&id);

    let (status, body) = request(
        &c,
        reqwest::Method::PATCH,
        &format!("/csourceRegistrations/{id}"),
        Some(&fragment),
        Some("application/ld+json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update to remove expiration (set to null)
// ---------------------------------------------------------------------------

/// PATCH expiresAt to null removes expiration -- 204 (5.9.3).
#[tokio::test]
async fn update_set_expires_at_to_null_removes_expiration() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-null-exp");
    let reg = csr_with_expiration(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let fragment = fragment_null_expires_at();
    let (status, _) = update_registration(&c, &id, &fragment, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("expiresAt").is_none(), "expiresAt should be absent after setting to null");

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update without ID in URL
// ---------------------------------------------------------------------------

/// PATCH /csourceRegistrations/ with no ID -- 405 (5.9.3).
#[tokio::test]
async fn update_without_id_in_url_returns_405() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-no-id");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mut fragment = fragment_different_type(&id);
    fragment.as_object_mut().unwrap().remove("id");
    let (status, _) = request(
        &c,
        reqwest::Method::PATCH,
        "/csourceRegistrations/",
        Some(&fragment),
        Some("application/ld+json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.5.7 -- The user @context an update is written under
// ---------------------------------------------------------------------------

/// Clause 5.5.7: a user `@context` declaring a JSON-LD Scoped Context is
/// `BadRequestData` on an update as on a create, and the registration is left
/// as it was.
#[tokio::test]
async fn update_under_a_scoped_user_context_returns_400_and_leaves_the_registration_unchanged() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-scoped");
    let (status, _, _) = create_registration(&c, &csr_basic_jsonld(&id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "endpoint": "http://scoped.example.org:1026",
        "@context": [{"speed": {"@id": "https://example.org/speed", "@context": {}}}, COMPOUND_CONTEXT_URL.as_str()]
    });
    let (status, body) = update_registration(&c, &id, &fragment, "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["endpoint"], "http://my.csource.org:1026/");

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.6: an update whose `@context` cannot be retrieved is
/// `LdContextNotAvailable`, and the registration is left as it was.
#[tokio::test]
async fn update_under_an_unreachable_context_returns_504_and_leaves_the_registration_unchanged() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-unreachable");
    let (status, _, _) = create_registration(&c, &csr_basic_jsonld(&id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = request(
        &c,
        Method::PATCH,
        &format!("/csourceRegistrations/{id}"),
        Some(&json!({"endpoint": "http://unreachable.example.org:1026"})),
        Some("application/json"),
        Some(UNREACHABLE_CONTEXT),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{body}");
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str());

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["endpoint"], "http://my.csource.org:1026/");

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}
