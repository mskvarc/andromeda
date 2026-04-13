#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Provision: Delete @context tests (NGSI-LD 6.25.3.2)

use reqwest::StatusCode;
use serial_test::serial;

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

/// 051_01_01: Delete hosted @context without reload param -> 204.
#[tokio::test]
#[serial]
async fn delete_hosted_context_returns_204() {
    clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    let (status, bytes) = delete_context_raw(&c, &ctx_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty(), "response body should be empty");
}

/// 051_02_01: Delete @context with unknown identifier -> 404.
#[tokio::test]
async fn delete_unknown_context_returns_404() {
    let c = client();
    let (status, body) = delete_context(&c, "urn:ngsi-ld:unknown:context:12345", "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
}

/// 051_04_01: Delete wrong ID + reload=true -> 400.
#[tokio::test]
async fn delete_unknown_with_reload_true_returns_400() {
    let c = client();
    let (status, _) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 051_04_02: Delete wrong ID + reload=false -> 404.
#[tokio::test]
async fn delete_unknown_with_reload_false_returns_404() {
    let c = client();
    let (status, _) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "reload=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 051_04_03: Delete wrong ID + no reload -> 404.
#[tokio::test]
async fn delete_unknown_no_reload_returns_404() {
    let c = client();
    let (status, _) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 051_04_04: Delete wrong ID + reload=xxx (invalid) -> 400.
#[tokio::test]
async fn delete_unknown_with_reload_invalid_returns_400() {
    let c = client();
    let (status, _) = delete_context(&c, "urn:ngsi-ld:unknown:ctx", "reload=xxx").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 051_04_05: Delete hosted + reload=true -> 400.
#[tokio::test]
#[serial]
async fn delete_hosted_with_reload_true_returns_400() {
    clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    let (status, _) = delete_context(&c, &ctx_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 051_06_01: Delete ImplicitlyCreated @context without reload -> 204.
#[tokio::test]
#[serial]
async fn delete_implicitly_created_context_returns_204() {
    clean_database().await;
    let c = client();
    let (sub_id, ctx_id) = create_implicit_context(&c).await;

    let (status, bytes) = delete_context_raw(&c, &ctx_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    crate::delete_subscription(&c, &sub_id).await;
    let _ = bytes;
}

/// 051_07_01: Delete ImplicitlyCreated @context with reload=true -> 400.
#[tokio::test]
#[serial]
async fn delete_implicitly_created_with_reload_returns_400() {
    clean_database().await;
    let c = client();
    let (sub_id, ctx_id) = create_implicit_context(&c).await;

    let (status, _) = delete_context(&c, &ctx_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);

    crate::delete_subscription(&c, &sub_id).await;
}

/// 051_08_01: Delete core context without reload -> 400.
#[tokio::test]
async fn delete_core_context_no_reload_returns_400() {
    let c = client();
    let (status, body) = delete_context(&c, CORE_CONTEXT, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

/// 051_08_02: Delete core context with reload=false -> 400.
#[tokio::test]
async fn delete_core_context_reload_false_returns_400() {
    let c = client();
    let (status, _) = delete_context(&c, CORE_CONTEXT, "reload=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 051_09_01: Delete core context with reload=true -> 204, then verify re-downloaded.
#[tokio::test]
async fn delete_and_reload_core_context_returns_204() {
    let c = client();

    // Verify it exists first
    let (status, _) = get_context(&c, CORE_CONTEXT, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Delete with reload
    let (status, _) = delete_context_raw(&c, CORE_CONTEXT, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify still accessible after reload
    let (status, _) = get_context(&c, CORE_CONTEXT, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
}

/// 051_03_01: Delete cached @context without reload -> 204.
#[tokio::test]
#[serial]
async fn delete_cached_context_returns_204() {
    clean_database().await;
    let c = client();
    let (_server, ctx_url) = setup_cached_context(&c).await;

    let cached_id = find_cached_context_id(&c, &ctx_url).await;

    let (status, bytes) = delete_context_raw(&c, &cached_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty());
}

/// 051_05_01: Delete cached @context with reload=true when server is unavailable -> 503.
#[tokio::test]
#[serial]
async fn delete_cached_reload_server_unavailable_returns_503() {
    clean_database().await;
    let c = client();
    let (mut server, ctx_url) = setup_cached_context(&c).await;

    let cached_id = find_cached_context_id(&c, &ctx_url).await;

    // Stop the context server
    server.stop();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Attempt delete with reload -- should fail since server is down
    let (status, body) = delete_context(&c, &cached_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE);
}
