#![allow(clippy::expect_used, clippy::unwrap_used)]
//! CSR update tests (NGSI-LD 5.9.3)

use reqwest::StatusCode;
use serial_test::serial;

use crate::{
    BAD_REQUEST_DATA_TYPE,
    INVALID_REQUEST_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    clean_database,
    cleanup_registration,
    client,
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

// ---------------------------------------------------------------------------
// 5.9.3 -- Update endpoint
// ---------------------------------------------------------------------------

/// PATCH endpoint field on existing CSR -- 204 (5.9.3).
#[tokio::test]
async fn update_endpoint_returns_204() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-endpoint");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let fragment = fragment_endpoint_update();
    let (status, _) = update_registration(&c, &id, &fragment, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_registration(&c, &id, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["endpoint"], "http://my.new.csource.org:1026/");

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update error cases
// ---------------------------------------------------------------------------

/// PATCH with invalid URI -- 400 (5.9.3).
#[tokio::test]
async fn update_invalid_uri_returns_400() {
    let c = client();
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
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

/// PATCH with body of different data type than existing registration -- 400 (5.9.3).
#[tokio::test]
async fn update_wrong_data_type_returns_400() {
    let c = client();
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
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);

    cleanup_registration(&c, &id).await;
}

/// PATCH attempting to remove mandatory property (information=null) -- 400 (5.9.3).
#[tokio::test]
async fn update_remove_mandatory_property_returns_400() {
    let c = client();
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
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);

    cleanup_registration(&c, &id).await;
}

/// PATCH with malformed JSON body -- 400 (5.9.3).
#[tokio::test]
async fn update_malformed_json_returns_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:ContextSourceRegistration:update-malformed");
    let reg = csr_basic_jsonld(&id);

    let (status, _, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let raw = csr_invalid_json_string();
    let (status, body) = update_registration_raw(&c, &id, raw.as_bytes(), "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
    assert!(body["title"].is_string(), "response should contain title");

    cleanup_registration(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update unknown registration
// ---------------------------------------------------------------------------

/// PATCH unknown registration -- 404 (5.9.3).
#[tokio::test]
async fn update_unknown_registration_returns_404() {
    let c = client();
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
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update to remove expiration (set to null)
// ---------------------------------------------------------------------------

/// PATCH expiresAt to null removes expiration -- 204 (5.9.3).
#[tokio::test]
#[serial]
async fn update_set_expires_at_to_null_removes_expiration() {
    clean_database().await;
    let c = client();
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
}

// ---------------------------------------------------------------------------
// 5.9.3 -- Update without ID in URL
// ---------------------------------------------------------------------------

/// PATCH /csourceRegistrations/ with no ID -- 405 (5.9.3).
#[tokio::test]
async fn update_without_id_in_url_returns_405() {
    let c = client();
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
}
