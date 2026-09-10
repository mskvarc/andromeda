#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Provision: Delete @context tests (NGSI-LD 6.25.3.2)

use reqwest::StatusCode;
use serial_test::serial;
use std::time::Duration;
use tokio::time::sleep;

use crate::{
    BAD_REQUEST_DATA_TYPE,
    CORE_CONTEXT,
    LD_CONTEXT_NOT_AVAILABLE_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    clean_database,
    client,
    create_hosted_context,
    create_implicit_context,
    delete_context,
    delete_context_raw,
    find_cached_context_id,
    get_context,
    setup_cached_context,
};

/// `051_01_01`: Delete hosted @context without reload param -> 204.
#[tokio::test]
#[serial]
async fn delete_hosted_context_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    let (status, bytes) = delete_context_raw(&c, &ctx_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty(), "response body should be empty");
}

/// `051_02_01`: Delete @context with unknown identifier -> 404.
#[tokio::test]
async fn delete_unknown_context_returns_404() {
    let c = client();
    let (status, body) = delete_context(&c, "urn:ngsi-ld:unknown:context:12345", "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
}

/// `051_04_01`: Delete wrong ID + reload=true -> 404.
///
/// Clause 5.13.5.4 settles the two refusals in order: "If the @context
/// identifier does not correspond to any existing entry in the @context
/// storage, then an error of type `ResourceNotFound` shall be raised" comes
/// before either reload rule, and an identifier that names nothing has no kind
/// for "only cached contexts can be reloaded" to object to. This case
/// previously expected `BadRequestData`, which reported a kind mismatch for a
/// Context that was never there.
#[tokio::test]
async fn delete_unknown_with_reload_true_returns_404() {
    let c = client();
    let (status, body) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
}

/// `051_04_02`: Delete wrong ID + reload=false -> 404.
#[tokio::test]
async fn delete_unknown_with_reload_false_returns_404() {
    let c = client();
    let (status, _) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "reload=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// `051_04_03`: Delete wrong ID + no reload -> 404.
#[tokio::test]
async fn delete_unknown_no_reload_returns_404() {
    let c = client();
    let (status, _) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// `051_04_04`: Delete wrong ID + reload=xxx (invalid) -> 400.
#[tokio::test]
async fn delete_unknown_with_reload_invalid_returns_400() {
    let c = client();
    let (status, _) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "reload=xxx").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// `051_04_05`: Delete hosted + reload=true -> 400.
#[tokio::test]
#[serial]
async fn delete_hosted_with_reload_true_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    let (status, _) = delete_context(&c, &ctx_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// `051_06_01`: Delete `ImplicitlyCreated` @context without reload -> 204.
#[tokio::test]
#[serial]
async fn delete_implicitly_created_context_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (sub_id, ctx_id) = create_implicit_context(&c).await;

    let (status, bytes) = delete_context_raw(&c, &ctx_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    crate::delete_subscription(&c, &sub_id).await;
    let _ = bytes;
}

/// `051_07_01`: Delete `ImplicitlyCreated` @context with reload=true -> 400.
#[tokio::test]
#[serial]
async fn delete_implicitly_created_with_reload_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (sub_id, ctx_id) = create_implicit_context(&c).await;

    let (status, _) = delete_context(&c, &ctx_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);

    crate::delete_subscription(&c, &sub_id).await;
}

/// `051_08_01`: Delete core context without reload -> 400.
#[tokio::test]
async fn delete_core_context_no_reload_returns_400() {
    let c = client();
    let (status, body) = delete_context(&c, CORE_CONTEXT, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

/// `051_08_02`: Delete core context with reload=false -> 400.
#[tokio::test]
async fn delete_core_context_reload_false_returns_400() {
    let c = client();
    let (status, _) = delete_context(&c, CORE_CONTEXT, "reload=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// `051_09_01`: Delete core context with reload=true -> 400, and it stays served.
///
/// Clause 5.13.5.4: "If reload is true and the kind of the @context is not
/// "Cached", implementations shall return a `BadRequestData` error." Clause
/// 5.13.1 flags the embedded core @context `ImplicitlyCreated`, so there is no
/// original URL to re-download it from — exactly as for the
/// `ImplicitlyCreated` Context that `051_07_01` covers. This case previously
/// expected `204`, claiming a re-download that never happened.
#[tokio::test]
async fn delete_and_reload_core_context_returns_400() {
    let c = client();

    // Verify it exists first
    let (status, _) = get_context(&c, CORE_CONTEXT, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let (status, body) = delete_context(&c, CORE_CONTEXT, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    // The refusal leaves the embedded core @context exactly where it was.
    let (status, _) = get_context(&c, CORE_CONTEXT, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
}

/// `051_03_01`: Delete cached @context without reload -> 204.
#[tokio::test]
#[serial]
async fn delete_cached_context_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (_server, ctx_url) = setup_cached_context(&c).await;

    let cached_id = find_cached_context_id(&c, &ctx_url).await;

    let (status, bytes) = delete_context_raw(&c, &cached_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(bytes, [] as [u8; 0]);
}

/// `051_05_01`: Delete cached @context with reload=true when server is unavailable -> 504.
#[tokio::test]
#[serial]
async fn delete_cached_reload_server_unavailable_returns_504() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (server, ctx_url) = setup_cached_context(&c).await;

    let cached_id = find_cached_context_id(&c, &ctx_url).await;

    // Stop the context server
    server.shutdown().await;
    sleep(Duration::from_millis(500)).await;

    // Attempt delete with reload -- should fail since server is down
    let (status, body) = delete_context(&c, &cached_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str());
}
