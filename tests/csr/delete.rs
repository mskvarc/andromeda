#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! CSR deletion tests (NGSI-LD 5.9.4)

use crate::{BAD_REQUEST_DATA_TYPE, create_registration, csr_basic_jsonld, delete_registration, request, retrieve_registration, unique_id};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use reqwest::StatusCode;

/// Delete existing CSR by ID -- 204, then retrieve returns 404 (5.9.4).
#[tokio::test]
async fn delete_existing_registration_returns_204() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:delete-existing");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_registration(&c, &id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}

/// DELETE /csourceRegistrations/ with no ID (empty path) -- 405 (5.9.4).
#[tokio::test]
async fn delete_without_id_returns_405() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, _) = request(&c, reqwest::Method::DELETE, "/csourceRegistrations/", None, None, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    scope.finish().await.expect("cleanup failed");
}

/// DELETE with invalid URI -- 400 (5.9.4).
#[tokio::test]
async fn delete_invalid_uri_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = delete_registration(&c, "invalidURI").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    assert!(body["title"].is_string(), "response should contain title");
    scope.finish().await.expect("cleanup failed");
}

/// DELETE unknown registration -- 404 (5.9.4).
#[tokio::test]
async fn delete_unknown_registration_returns_404() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:delete-unknown");
    let (status, body) = delete_registration(&c, &id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["title"].is_string(), "response should contain title");
    scope.finish().await.expect("cleanup failed");
}
