#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Consumption: Serve @context tests (NGSI-LD 6.25.3.4)

use andromeda_test_mocks::{callback_path::CallbackPath, jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::{body::Bytes, http::HeaderMap};
use reqwest::StatusCode;
use std::{collections::HashMap, time::Duration};
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    ContextServer,
    LD_CONTEXT_NOT_AVAILABLE_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    broker_url,
    create_entity,
    create_hosted_context,
    create_subscription,
    delete_entity,
    delete_subscription,
    extract_id,
    find_cached_context_id,
    get_context,
    list_contexts,
    number_of_hits,
    provision_tenant,
    setup_cached_context,
    tenant_client,
    unique_id,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use serde_json::{Value, json};

/// `053_01_01`: Serve @context without details parameter -> returns @context body.
#[tokio::test]
async fn serve_context_no_details_returns_context_body() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let ctx_id = create_hosted_context(&c).await;

    let (status, body) = get_context(&c, &ctx_id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ctx = body.get("@context").expect("response should contain @context");
    pretty_assertions::assert_eq!(ctx["A1"], "urn:ngsi-ld:attributes:A1");
    pretty_assertions::assert_eq!(ctx["A2"], "urn:ngsi-ld:attributes:A2");
    pretty_assertions::assert_eq!(ctx["A3"], "urn:ngsi-ld:attributes:A3");
    env.finish().await.expect("cleanup failed");
}

/// `053_01_02`: Serve @context with details=false -> returns @context body.
#[tokio::test]
async fn serve_context_details_false_returns_context_body() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let ctx_id = create_hosted_context(&c).await;

    let (status, body) = get_context(&c, &ctx_id, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let ctx = body.get("@context").expect("response should contain @context");
    pretty_assertions::assert_eq!(ctx["A1"], "urn:ngsi-ld:attributes:A1");
    env.finish().await.expect("cleanup failed");
}

/// `053_02_01`: Serve @context with details=true -> returns metadata.
#[tokio::test]
async fn serve_context_details_true_returns_metadata() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let ctx_id = create_hosted_context(&c).await;

    let (status, body) = get_context(&c, &ctx_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("URL").is_some(), "detailed response should have URL");
    pretty_assertions::assert_eq!(body["kind"], "Hosted");
    assert!(body.get("createdAt").is_some(), "detailed response should have createdAt");
    env.finish().await.expect("cleanup failed");
}

/// `053_03_01`: Serve @context with unknown ID -> 404.
#[tokio::test]
async fn serve_unknown_context_returns_404() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, body) = get_context(&c, "urn:ngsi-ld:unknown:ctx:12345", "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    env.finish().await.expect("cleanup failed");
}

/// `053_04_01`: numberOfHits increases after using hosted context.
#[tokio::test]
async fn serve_hosted_number_of_hits_increases() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let ctx_id = create_hosted_context(&c).await;

    // Check initial numberOfHits
    let (status, body) = get_context(&c, &ctx_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let initial_hits = body["numberOfHits"].as_u64().unwrap_or(0);
    let context_url = body["URL"].as_str().expect("should have URL").to_string();

    // Create entity using this context to increment hits
    let entity_id = unique_id("urn:ngsi-ld:Testing:hits");
    let entity = json!({
        "id": &entity_id,
        "type": "Testing",
        "A1": {"type": "Property", "value": "value of A1"},
        "@context": [&context_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // The hit lands in the served count immediately: clause 5.13.3.5 read is
    // folded with this process's un-flushed accesses, so a read taken right
    // after the create -- with no wait for the background flush -- already
    // reflects the increase.
    let new_hits = number_of_hits(&c, &ctx_id).await;
    assert!(new_hits > initial_hits, "numberOfHits must increase immediately: {new_hits} > {initial_hits}");

    delete_entity(&c, &entity_id).await;
    env.finish().await.expect("cleanup failed");
}

/// `053_07_01`: Serve `ImplicitlyCreated` @context with details=true.
#[tokio::test]
async fn serve_implicitly_created_details_true() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;

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
    env.finish().await.expect("cleanup failed");
}

/// `053_08_01`: numberOfHits increases after using `ImplicitlyCreated` context.
#[tokio::test]
async fn serve_implicitly_created_number_of_hits_increases() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;

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
    let context_url = body["URL"].as_str().expect("should have URL").to_string();

    // Create entity using this context
    let entity_id = unique_id("urn:ngsi-ld:Testing:impl-hits");
    let entity = json!({
        "id": &entity_id,
        "type": "Testing",
        "A1": {"type": "Property", "value": "value"},
        "@context": [&context_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Read immediately, with no wait for the background flush: the served count
    // folds this process's un-flushed accesses (clause 5.13.3.5).
    let new_hits = number_of_hits(&c, &ctx_id).await;
    assert!(new_hits > initial_hits, "numberOfHits must increase immediately: {new_hits} > {initial_hits}");

    delete_entity(&c, &entity_id).await;
    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

/// `053_05_01`: Cached context still available after context server goes down.
#[tokio::test]
async fn serve_cached_context_after_server_down() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (server, context_url) = setup_cached_context(&c).await;

    let cached_id = find_cached_context_id(&c, &context_url).await;

    // Stop the context server
    server.shutdown().await;
    sleep(Duration::from_millis(500)).await;

    // Attempt delete with reload (should fail)
    let (status, _) = crate::delete_context(&c, &cached_id, "reload=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);

    // But GET should still work from cache
    let (status, body) = get_context(&c, &cached_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["kind"], "Cached");
    assert!(body.get("URL").is_some());
    assert!(body.get("createdAt").is_some());
    env.finish().await.expect("cleanup failed");
}

/// A remote `@context` resolved once is recorded as a durable `Cached` entry, so
/// a second resolution in the same tenant reads it from storage rather than
/// fetching it again: the mock is contacted exactly once for two entity creates,
/// and the context is listed `Cached` (clauses 5.13.1, 5.13.3).
#[tokio::test]
async fn serve_remote_context_second_resolution_does_not_refetch() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;

    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let path = CallbackPath::parse(format!("/second-resolution-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let context_url = server
        .serve(
            path.clone(), // clone: the server retains the route while the test reads its request count.
            Bytes::from_static(br#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#),
            HeaderMap::new(),
        )
        .await
        .expect("context route must be configured")
        .to_string();

    for suffix in ["a", "b"] {
        let entity = json!({
            "id": unique_id(&format!("urn:ngsi-ld:Testing:second-resolution-{suffix}")),
            "type": "Testing",
            "X1": {"type": "Property", "value": suffix},
            "@context": [&context_url]
        });
        let (status, _) = create_entity(&c, &entity).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    pretty_assertions::assert_eq!(
        server.request_count(&path),
        1,
        "a durably Cached remote context is fetched exactly once for both resolutions"
    );

    let (status, contexts) = list_contexts(&c, "details=true&kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let cached = contexts
        .iter()
        .find(|entry| entry.get("URL").and_then(|url| url.as_str()) == Some(context_url.as_str()));
    assert!(cached.is_some(), "the resolved remote context is listed as Cached: {contexts:?}");
    pretty_assertions::assert_eq!(cached.expect("cached entry")["kind"], "Cached");

    server.shutdown().await.expect("context server shutdown failed");
    env.finish().await.expect("cleanup failed");
}

/// A cold remote `@context` -- one never cached -- whose origin is unreachable
/// fails the referencing operation with `504 LdContextNotAvailable`: the loader
/// retries a transport failure a bounded number of times, then reports the
/// deterministic unavailability (Table 6.3.2-1, clause 5.13).
#[tokio::test]
async fn serve_remote_context_unavailable_maps_to_504() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;

    // Serve a context, take its URL, then shut the origin down before anything
    // references it, so the resolution is cold and must fetch and fail.
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let path = CallbackPath::parse(format!("/unavailable-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let context_url = server
        .serve(
            path,
            Bytes::from_static(br#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#),
            HeaderMap::new(),
        )
        .await
        .expect("context route must be configured")
        .to_string();
    server.shutdown().await.expect("context server shutdown failed");
    sleep(Duration::from_millis(500)).await;

    let entity = json!({
        "id": unique_id("urn:ngsi-ld:Testing:unavailable"),
        "type": "Testing",
        "X1": {"type": "Property", "value": "cold"},
        "@context": [&context_url]
    });
    let response = c
        .post(broker_url("entities"))
        .header("Content-Type", "application/ld+json")
        .json(&entity)
        .send()
        .await
        .expect("create entity request");
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    pretty_assertions::assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "a cold unavailable @context is a 504: {body}");
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str());
    env.finish().await.expect("cleanup failed");
}

/// `053_06_01`: numberOfHits increases for cached context.
#[tokio::test]
async fn serve_cached_number_of_hits_increases() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;

    let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
    let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
    let context_url = server.url("/context.jsonld");

    // Create first entity
    let entity_id1 = unique_id("urn:ngsi-ld:Testing:cache-hits-1");
    let entity = json!({
        "id": &entity_id1,
        "type": "Testing",
        "X1": {"type": "Property", "value": "test1"},
        "@context": [&context_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // The baseline is read immediately after the first create. The very first
    // remote fetch is not itself a cache hit (clause 5.13.3.5), so this may read
    // zero; whatever it reads is exact, because the served count already folds
    // this process's un-flushed accesses rather than waiting for the flush.
    let cached_id = find_cached_context_id(&c, &context_url).await;
    let hits_after_first = number_of_hits(&c, &cached_id).await;

    // Create second entity using same context
    let entity_id2 = unique_id("urn:ngsi-ld:Testing:cache-hits-2");
    let entity = json!({
        "id": &entity_id2,
        "type": "Testing",
        "X1": {"type": "Property", "value": "test2"},
        "@context": [&context_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // The second create is a cache hit, and it lands in the served count at
    // once -- no flush wait.
    let hits_after_second = number_of_hits(&c, &cached_id).await;
    assert!(
        hits_after_second > hits_after_first,
        "numberOfHits must increase immediately: {hits_after_second} > {hits_after_first}"
    );

    delete_entity(&c, &entity_id1).await;
    delete_entity(&c, &entity_id2).await;
    let _ = server;
    env.finish().await.expect("cleanup failed");
}
