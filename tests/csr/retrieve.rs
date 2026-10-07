#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! CSR retrieval tests (NGSI-LD 5.10.1, 6.3.5)

use crate::{
    BAD_REQUEST_DATA_TYPE,
    COMPOUND_CONTEXT_URL,
    RESOURCE_NOT_FOUND_TYPE,
    cleanup_registration,
    create_registration,
    csr_basic_jsonld,
    expected_csr_compacted,
    expected_csr_expanded,
    retrieve_registration,
    strip_ignored_keys,
    unique_id,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use reqwest::StatusCode;

/// Retrieve with non-URI ID -- 400 (5.10.1).
#[tokio::test]
async fn retrieve_invalid_id_returns_400() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = retrieve_registration(&c, "invalidUri", None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve non-existent registration -- 404 (5.10.1).
#[tokio::test]
async fn retrieve_unknown_registration_returns_404() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let (status, body) = retrieve_registration(&c, "urn:ngsi-ld:ContextSourceRegistration:unknownRegistration", None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve without Link header -- custom terms rendered as FQNs (5.10.1, 5.5.7).
#[tokio::test]
async fn retrieve_without_context_returns_expanded_format() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-expanded");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_csr_expanded(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve with compound context Link header -- compacted names (5.10.1, 5.5.7).
#[tokio::test]
async fn retrieve_with_context_returns_compacted_format() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-compacted");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_csr_compacted(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Empty JSON-LD context (no Link header) -- same as expanded format (6.3.5).
#[tokio::test]
async fn retrieve_empty_jsonld_context_returns_expanded_format() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-empty-ctx");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_csr_expanded(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Creation-time JSON-LD context with Link header -- same as compacted format (6.3.5).
#[tokio::test]
async fn retrieve_creation_time_context_returns_compacted_format() {
    let scope = TenantScopedEnvironment::from_suite("csr").await.expect("tenant-scoped test environment");
    let c = scope.client().clone();
    crate::provision_tenant(&c).await;
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-creation-ctx");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = expected_csr_compacted(&id);
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}
