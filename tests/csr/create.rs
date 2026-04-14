#![allow(clippy::expect_used, clippy::unwrap_used)]
//! CSR creation tests (NGSI-LD 5.9.2, 6.3.5)

use reqwest::StatusCode;

use crate::{
    BAD_REQUEST_DATA_TYPE,
    COMPOUND_CONTEXT_URL,
    cleanup_registration,
    client,
    create_registration,
    csr_basic_json,
    csr_basic_jsonld,
    csr_invalid_null_information,
    csr_invalid_structure,
    csr_no_id,
    csr_past_expiration,
    csr_with_expiration,
    csr_with_location,
    csr_with_mode_and_operations,
    request,
    retrieve_registration,
    strip_ignored_keys,
    unique_id,
};

// ---------------------------------------------------------------------------
// 5.9.2 -- Basic creation
// ---------------------------------------------------------------------------

/// Create CSR without specifying mode or operations -- defaults applied (5.9.2).
#[tokio::test]
async fn create_without_mode_and_operations_returns_201() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-basic");
    let reg = csr_basic_jsonld(&id);

    let (status, location, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let loc = location.expect("should have Location header");
    assert!(loc.contains(&id), "Location should contain registration ID");

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status", "mode", "operations"];
    let mut expected = reg;
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}

/// Create CSR with explicit mode and operations (5.9.2).
#[tokio::test]
async fn create_with_mode_and_operations_returns_201() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-mode-ops");
    let reg = csr_with_mode_and_operations(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = reg;
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}

/// Create CSR with location geometry (5.9.2).
#[tokio::test]
async fn create_with_location_returns_201() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-location");
    let reg = csr_with_location(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status", "mode", "operations"];
    let mut expected = reg;
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}

/// Create CSR with future expiration date (5.9.2).
#[tokio::test]
async fn create_with_expiration_returns_201() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-expiration");
    let reg = csr_with_expiration(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status", "mode", "operations"];
    let mut expected = reg;
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    pretty_assertions::assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}

/// Create CSR without specifying an ID -- server generates one (5.9.2).
#[tokio::test]
async fn create_without_id_server_generates_id() {
    let c = client();
    let reg = csr_no_id();

    let (status, location, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let loc = location.expect("should have Location header");
    assert!(!loc.is_empty(), "Location header should not be empty");
    let id = loc.rsplit('/').next().expect("Location should contain an ID");

    let (status, body) = retrieve_registration(&c, id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body["id"].is_string(), "response should contain id");

    cleanup_registration(&c, id).await;
}

// ---------------------------------------------------------------------------
// 5.9.2 -- Invalid content
// ---------------------------------------------------------------------------

/// Create CSR with null information field -- 400 (5.9.2).
#[tokio::test]
async fn create_with_invalid_content_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-invalid");
    let reg = csr_invalid_null_information(&id);

    let (status, _, body) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
    assert!(body["title"].is_string(), "response should contain title");
}

/// Create CSR with invalid structure (missing information) -- 400 (5.9.2).
#[tokio::test]
async fn create_with_invalid_structure_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-invalid-struct");
    let reg = csr_invalid_structure(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Create CSR with expiration date in the past -- 400 (5.9.2).
#[tokio::test]
async fn create_with_past_expiration_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-past-exp");
    let reg = csr_past_expiration(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// 5.9.2 -- Duplicate registration
// ---------------------------------------------------------------------------

/// Create CSR that already exists -- 409 (5.9.2).
#[tokio::test]
async fn create_duplicate_returns_409() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-dup");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["title"].is_string(), "response should contain title");

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 6.3.5 -- JSON content type with Link header context
// ---------------------------------------------------------------------------

/// JSON body + Link header context, retrieve with context -- compacted names (6.3.5).
#[tokio::test]
async fn create_json_with_link_context_retrieve_with_context() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-json-link");
    let mut reg = csr_basic_json(&id);
    reg.as_object_mut().unwrap().remove("@context");

    let (status, _) = request(
        &c,
        reqwest::Method::POST,
        "/csourceRegistrations/",
        Some(&reg),
        Some("application/json"),
        Some(COMPOUND_CONTEXT_URL),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["information"][0]["entities"][0]["type"], "Building");

    cleanup_registration(&c, &id).await;
}

/// JSON body + Link header context, retrieve without context -- expanded FQNs (6.3.5).
#[tokio::test]
async fn create_json_with_link_context_retrieve_without_context_returns_fqn() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-json-link-noret");
    let mut reg = csr_basic_json(&id);
    reg.as_object_mut().unwrap().remove("@context");

    let (status, _) = request(
        &c,
        reqwest::Method::POST,
        "/csourceRegistrations/",
        Some(&reg),
        Some("application/json"),
        Some(COMPOUND_CONTEXT_URL),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["information"][0]["entities"][0]["type"], "https://ngsi-ld-test-suite/context#Building");

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 6.3.5 -- Default context with JSON content
// ---------------------------------------------------------------------------

/// JSON body without Link header (default core context), retrieve with compound context (6.3.5).
/// Core @vocab expands "Building" to "ngsi-ld:default-context/Building" -- compound context
/// cannot compact it back since its mapping points elsewhere.
#[tokio::test]
async fn create_json_default_context_retrieve_with_compound_context() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-json-default");
    let mut reg = csr_basic_json(&id);
    reg.as_object_mut().unwrap().remove("@context");

    let (status, _) = request(
        &c,
        reqwest::Method::POST,
        "/csourceRegistrations/",
        Some(&reg),
        Some("application/json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["information"][0]["entities"][0]["type"], "ngsi-ld:default-context/Building");

    cleanup_registration(&c, &id).await;
}

/// JSON body without Link header, retrieve without context -- short names via core @context (6.3.5).
#[tokio::test]
async fn create_json_default_context_retrieve_without_context() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-json-default-noret");
    let mut reg = csr_basic_json(&id);
    reg.as_object_mut().unwrap().remove("@context");

    let (status, _) = request(
        &c,
        reqwest::Method::POST,
        "/csourceRegistrations/",
        Some(&reg),
        Some("application/json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["information"][0]["entities"][0]["type"], "Building");

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 6.3.5 -- JSON content type with @context in body (should be rejected)
// ---------------------------------------------------------------------------

/// application/json body containing @context key -- 400 (6.3.5).
#[tokio::test]
async fn create_json_content_type_with_context_in_body_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-json-with-ctx");
    let reg = csr_basic_jsonld(&id);

    let (status, body) = request(
        &c,
        reqwest::Method::POST,
        "/csourceRegistrations/",
        Some(&reg),
        Some("application/json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
    assert!(body["title"].is_string(), "response should contain title");
}

// ---------------------------------------------------------------------------
// 6.3.5 -- JSON-LD content type
// ---------------------------------------------------------------------------

/// JSON-LD body with @context, retrieve with compound context -- compacted names (6.3.5).
#[tokio::test]
async fn create_jsonld_retrieve_with_context_returns_compacted_type() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-ld-with-ctx");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["information"][0]["entities"][0]["type"], "Vehicle");

    cleanup_registration(&c, &id).await;
}

/// JSON-LD body with @context, retrieve without context -- expanded FQNs (6.3.5).
#[tokio::test]
async fn create_jsonld_retrieve_without_context_returns_expanded_type() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-ld-no-ctx");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["information"][0]["entities"][0]["type"], "https://ngsi-ld-test-suite/context#Vehicle");

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 6.3.5 -- JSON-LD content type without @context
// ---------------------------------------------------------------------------

/// application/ld+json without @context in body -- 400 (6.3.5).
#[tokio::test]
async fn create_jsonld_content_type_without_context_in_body_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-ld-no-body-ctx");
    let mut reg = csr_basic_json(&id);
    reg.as_object_mut().unwrap().remove("@context");

    let (status, body) = request(
        &c,
        reqwest::Method::POST,
        "/csourceRegistrations/",
        Some(&reg),
        Some("application/ld+json"),
        None,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
    assert!(body["title"].is_string(), "response should contain title");
}

// ---------------------------------------------------------------------------
// 6.3.5 -- JSON-LD content type with Link header (should be rejected)
// ---------------------------------------------------------------------------

/// application/ld+json body AND Link header context -- 400 (6.3.5).
#[tokio::test]
async fn create_jsonld_content_type_with_link_header_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:create-ld-link");
    let reg = csr_basic_jsonld(&id);

    let (status, body) = request(
        &c,
        reqwest::Method::POST,
        "/csourceRegistrations/",
        Some(&reg),
        Some("application/ld+json"),
        Some(COMPOUND_CONTEXT_URL),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
    assert!(body["title"].is_string(), "response should contain title");
}
