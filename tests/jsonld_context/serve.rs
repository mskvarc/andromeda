#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Consumption: Serve @context tests (NGSI-LD 6.25.3.4)

use reqwest::StatusCode;
use serial_test::serial;
use std::collections::HashMap;

use crate::{
    ContextServer,
    RESOURCE_NOT_FOUND_TYPE,
    clean_database,
    client,
    create_entity,
    create_hosted_context,
    create_subscription,
    delete_entity,
    delete_subscription,
    extract_id,
    find_cached_context_id,
    get_context,
    list_contexts,
    setup_cached_context,
    unique_id,
    wait_for_hits_above,
};
use serde_json::json;

/// 053_01_01: Serve @context without details parameter -> returns @context body.
#[tokio::test]
#[serial]
async fn serve_context_no_details_returns_context_body() {
    clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    let (status, body) = get_context(&c, &ctx_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ctx = body.get("@context").expect("response should contain @context");
    pretty_assertions::assert_eq!(ctx["A1"], "urn:ngsi-ld:attributes:A1");
    pretty_assertions::assert_eq!(ctx["A2"], "urn:ngsi-ld:attributes:A2");
    pretty_assertions::assert_eq!(ctx["A3"], "urn:ngsi-ld:attributes:A3");
}

/// 053_01_02: Serve @context with details=false -> returns @context body.
#[tokio::test]
#[serial]
async fn serve_context_details_false_returns_context_body() {
    clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    let (status, body) = get_context(&c, &ctx_id, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ctx = body.get("@context").expect("response should contain @context");
    pretty_assertions::assert_eq!(ctx["A1"], "urn:ngsi-ld:attributes:A1");
}

/// 053_02_01: Serve @context with details=true -> returns metadata.
#[tokio::test]
#[serial]
async fn serve_context_details_true_returns_metadata() {
    clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    let (status, body) = get_context(&c, &ctx_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("URL").is_some(), "detailed response should have URL");
    pretty_assertions::assert_eq!(body["kind"], "Hosted");
    assert!(body.get("createdAt").is_some(), "detailed response should have createdAt");
}

/// 053_03_01: Serve @context with unknown ID -> 404.
#[tokio::test]
async fn serve_unknown_context_returns_404() {
    let c = client();
    let (status, body) = get_context(&c, "urn:ngsi-ld:unknown:ctx:12345", "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
}

/// 053_04_01: numberOfHits increases after using hosted context.
#[tokio::test]
#[serial]
async fn serve_hosted_number_of_hits_increases() {
    clean_database().await;
    let c = client();
    let ctx_id = create_hosted_context(&c).await;

    // Check initial numberOfHits
    let (status, body) = get_context(&c, &ctx_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let initial_hits = body["numberOfHits"].as_u64().unwrap_or(0);
    let ctx_url = body["URL"].as_str().expect("should have URL").to_string();

    // Create entity using this context to increment hits
    let entity_id = unique_id("urn:ngsi-ld:Testing:hits");
    let entity = json!({
        "id": &entity_id,
        "type": "Testing",
        "A1": {"type": "Property", "value": "value of A1"},
        "@context": [&ctx_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Check numberOfHits increased. The counter is flushed to the row by a
    // background task, not by the create, so this waits for that flush.
    let new_hits = wait_for_hits_above(&c, &ctx_id, initial_hits).await;
    assert!(new_hits > initial_hits, "numberOfHits should have increased: {new_hits} > {initial_hits}");

    delete_entity(&c, &entity_id).await;
}

/// 053_07_01: Serve ImplicitlyCreated @context with details=true.
#[tokio::test]
#[serial]
async fn serve_implicitly_created_details_true() {
    clean_database().await;
    let c = client();

    let sub = json!({
        "type": "Subscription",
        "entities": [{"type": "T1"}],
        "notification": {"endpoint": {"uri": "http://a.b.c/123"}},
        "@context": [
            "https://fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld",
            "http://smartdatamodels.org/context.jsonld"
        ]
    });
    let (status, location) = create_subscription(&c, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let sub_id = extract_id(location.as_ref().unwrap());

    // Find the implicit context
    let (_, contexts) = list_contexts(&c, "kind=ImplicitlyCreated").await;
    let ctx_uri = contexts.first().expect("should have implicit context").as_str().unwrap();
    let ctx_id = extract_id(ctx_uri);

    // Serve with details
    let (status, body) = get_context(&c, &ctx_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["kind"], "ImplicitlyCreated");
    assert!(body.get("URL").is_some());
    assert!(body.get("createdAt").is_some());

    delete_subscription(&c, &sub_id).await;
}

/// 053_08_01: numberOfHits increases after using ImplicitlyCreated context.
#[tokio::test]
#[serial]
async fn serve_implicitly_created_number_of_hits_increases() {
    clean_database().await;
    let c = client();

    let sub = json!({
        "type": "Subscription",
        "entities": [{"type": "T1"}],
        "notification": {"endpoint": {"uri": "http://a.b.c/123"}},
        "@context": [
            "https://fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld",
            "http://smartdatamodels.org/context.jsonld"
        ]
    });
    let (status, location) = create_subscription(&c, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let sub_id = extract_id(location.as_ref().unwrap());

    // Find implicit context
    let (_, contexts) = list_contexts(&c, "kind=ImplicitlyCreated").await;
    let ctx_uri = contexts.first().expect("should have implicit context").as_str().unwrap();
    let ctx_id = extract_id(ctx_uri);

    // Check initial hits
    let (_, body) = get_context(&c, &ctx_id, "details=true").await;
    let initial_hits = body["numberOfHits"].as_u64().unwrap_or(0);
    let ctx_url = body["URL"].as_str().expect("should have URL").to_string();

    // Create entity using this context
    let entity_id = unique_id("urn:ngsi-ld:Testing:impl-hits");
    let entity = json!({
        "id": &entity_id,
        "type": "Testing",
        "A1": {"type": "Property", "value": "value"},
        "@context": [&ctx_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Check hits increased, once the background flush has folded them in.
    let new_hits = wait_for_hits_above(&c, &ctx_id, initial_hits).await;
    assert!(new_hits > initial_hits, "numberOfHits should increase: {new_hits} > {initial_hits}");

    delete_entity(&c, &entity_id).await;
    delete_subscription(&c, &sub_id).await;
}

/// 053_05_01: Cached context still available after context server goes down.
#[tokio::test]
#[serial]
async fn serve_cached_context_after_server_down() {
    clean_database().await;
    let c = client();
    let (mut server, ctx_url) = setup_cached_context(&c).await;

    let cached_id = find_cached_context_id(&c, &ctx_url).await;

    // Stop the context server
    server.stop();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Attempt delete with reload (should fail)
    let (status, _) = crate::delete_context(&c, &cached_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // But GET should still work from cache
    let (status, body) = get_context(&c, &cached_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["kind"], "Cached");
    assert!(body.get("URL").is_some());
    assert!(body.get("createdAt").is_some());
}

/// 053_06_01: numberOfHits increases for cached context.
#[tokio::test]
#[serial]
async fn serve_cached_number_of_hits_increases() {
    clean_database().await;
    let c = client();

    let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
    let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
    let ctx_url = server.url("/context.jsonld");

    // Create first entity
    let entity_id1 = unique_id("urn:ngsi-ld:Testing:cache-hits-1");
    let entity = json!({
        "id": &entity_id1,
        "type": "Testing",
        "X1": {"type": "Property", "value": "test1"},
        "@context": [&ctx_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Find cached context and take the baseline only once the first entity's
    // hit is on the row. Reading before the flush would leave the baseline at
    // zero, and the second read could then pass on the first entity's hit
    // alone without the second one ever being counted.
    let cached_id = find_cached_context_id(&c, &ctx_url).await;
    let hits_after_first = wait_for_hits_above(&c, &cached_id, 0).await;

    // Create second entity using same context
    let entity_id2 = unique_id("urn:ngsi-ld:Testing:cache-hits-2");
    let entity = json!({
        "id": &entity_id2,
        "type": "Testing",
        "X1": {"type": "Property", "value": "test2"},
        "@context": [&ctx_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Check hits increased
    let hits_after_second = wait_for_hits_above(&c, &cached_id, hits_after_first).await;
    assert!(
        hits_after_second > hits_after_first,
        "numberOfHits should increase: {hits_after_second} > {hits_after_first}"
    );

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    let _ = server;
}
