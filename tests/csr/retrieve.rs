//! CSR retrieval tests (NGSI-LD 5.10.1, 6.3.5)

use reqwest::StatusCode;

use crate::{
    BAD_REQUEST_DATA_TYPE,
    COMPOUND_CONTEXT_URL,
    RESOURCE_NOT_FOUND_TYPE,
    cleanup_registration,
    client,
    create_registration,
    load_expectation,
    load_fixture,
    retrieve_registration,
    strip_ignored_keys,
    unique_id,
};
use serde_json::Value;

// ---------------------------------------------------------------------------
// 5.10.1 -- Retrieve with invalid ID
// ---------------------------------------------------------------------------

/// Retrieve with non-URI ID -- 400 (5.10.1).
#[tokio::test]
async fn retrieve_invalid_id_returns_400() {
    let c = client();
    let (status, body) = retrieve_registration(&c, "invalidUri", None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

// ---------------------------------------------------------------------------
// 5.10.1 -- Retrieve unknown registration
// ---------------------------------------------------------------------------

/// Retrieve non-existent registration -- 404 (5.10.1).
#[tokio::test]
async fn retrieve_unknown_registration_returns_404() {
    let c = client();
    let (status, body) = retrieve_registration(&c, "urn:ngsi-ld:ContextSourceRegistration:unknownRegistration", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
}

// ---------------------------------------------------------------------------
// 5.10.1, 5.5.7 -- Retrieve with default core context (no Link header)
// ---------------------------------------------------------------------------

/// Retrieve without Link header -- custom terms rendered as FQNs (5.10.1, 5.5.7).
#[tokio::test]
async fn retrieve_without_context_returns_expanded_format() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-expanded");
    let reg = load_fixture("csource_registrations/context-source-registration.jsonld", &id);

    let (status, _, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, None, None).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registration-expanded-format.json");
    expected["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.10.1, 5.5.7 -- Retrieve with Link header context
// ---------------------------------------------------------------------------

/// Retrieve with compound context Link header -- compacted names (5.10.1, 5.5.7).
#[tokio::test]
async fn retrieve_with_context_returns_compacted_format() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-compacted");
    let reg = load_fixture("csource_registrations/context-source-registration.jsonld", &id);

    let (status, _, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registration.json");
    expected["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 6.3.5 -- JSON-LD context resolution
// ---------------------------------------------------------------------------

/// Empty JSON-LD context (no Link header) -- same as expanded format (6.3.5).
#[tokio::test]
async fn retrieve_empty_jsonld_context_returns_expanded_format() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-empty-ctx");
    let reg = load_fixture("csource_registrations/context-source-registration.jsonld", &id);

    let (status, _, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, None, None).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registration-expanded-format.json");
    expected["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}

/// Creation-time JSON-LD context with Link header -- same as compacted format (6.3.5).
#[tokio::test]
async fn retrieve_creation_time_context_returns_compacted_format() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:retrieve-creation-ctx");
    let reg = load_fixture("csource_registrations/context-source-registration.jsonld", &id);

    let (status, _, _) = create_registration(&c, &reg).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, mut actual) = retrieve_registration(&c, &id, Some("application/ld+json"), Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(status, StatusCode::OK);

    let ignored = &["@context", "createdAt", "modifiedAt", "status"];
    let mut expected = load_expectation("csource_registrations/expectations/context-source-registration.json");
    expected["id"] = Value::String(id.clone());
    strip_ignored_keys(&mut actual, ignored);
    strip_ignored_keys(&mut expected, ignored);
    assert_eq!(expected, actual);

    cleanup_registration(&c, &id).await;
}
