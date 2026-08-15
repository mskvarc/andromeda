#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Provision: Add @context tests (NGSI-LD 6.25.3.1)

use reqwest::StatusCode;
use serial_test::serial;
use std::collections::HashMap;

use crate::{
    ContextServer,
    INVALID_REQUEST_TYPE,
    clean_database,
    client,
    create_context,
    create_entity,
    create_subscription,
    delete_entity,
    delete_subscription,
    extract_id,
    get_context,
    list_contexts,
    unique_id,
};
use serde_json::json;

/// 050_01_01: Add valid hosted @context from key-value dictionary -> 201.
#[tokio::test]
#[serial]
async fn add_valid_context_dictionary_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, location, resp_body) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some(), "should have Location header");

    // Verify the context was created as Hosted
    let ctx_id = extract_id(location.as_ref().unwrap());
    let (status, detail) = get_context(&c, &ctx_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(detail["kind"], "Hosted");

    let _ = resp_body;
}

/// 050_01_02: Add valid hosted @context from URI list -> 201.
#[tokio::test]
#[serial]
async fn add_valid_context_uri_list_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let body = json!({
        "@context": [
            "http://smartdatamodels.org/context.jsonld",
            "https://fiware.github.io/data-models/context.jsonld"
        ]
    });
    let (status, location, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some(), "should have Location header");
}

/// 050_02_01: Add @context with incorrect payload (missing @context key) -> 400.
#[tokio::test]
async fn add_invalid_context_missing_key_returns_400() {
    let c = client();

    // No @context key -- just bare attributes
    let body = json!({
        "A1": "urn:ngsi-ld:attributes:A1",
        "A2": "urn:ngsi-ld:attributes:A2",
        "A3": "urn:ngsi-ld:attributes:A3"
    });
    let (status, _, body) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE.as_str());
}

/// 050_02_02: Add @context with malformed JSON (empty object) -> 400.
#[tokio::test]
async fn add_invalid_context_empty_object_returns_400() {
    let c = client();

    let body = json!({});
    let (status, _, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 050_03_01: ImplicitlyCreated @context via subscription creation.
#[tokio::test]
#[serial]
async fn add_implicitly_created_context_via_subscription() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let sub = json!({
        "type": "Subscription",
        "entities": [{"type": "T1"}],
        "notification": {
            "endpoint": {"uri": "http://a.b.c/123"}
        },
        "@context": [
            "https://fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld",
            "http://smartdatamodels.org/context.jsonld"
        ]
    });

    let (status, location) = create_subscription(&c, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let sub_id = extract_id(location.as_ref().unwrap());

    // List contexts with kind=ImplicitlyCreated
    let (status, contexts) = list_contexts(&c, "kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!contexts.is_empty(), "should have at least one ImplicitlyCreated context");

    delete_subscription(&c, &sub_id).await;
}

/// 050_04_01: Add hosted @context with URI references, verify URIs become cached.
#[tokio::test]
#[serial]
async fn add_hosted_context_with_cached_uri_refs() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
    let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
    let ctx_url = server.url("/context.jsonld");

    // Create a hosted context that references the local server URL
    let body = json!({"@context": [ctx_url]});
    let (status, location, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let hosted_id = extract_id(location.as_ref().unwrap());

    // Verify the hosted context kind
    let (status, detail) = get_context(&c, &hosted_id, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(detail["kind"], "Hosted");

    // Use the full context URL from the detail response
    let hosted_url = detail["URL"].as_str().expect("should have URL").to_string();

    // Create entity using the hosted context to trigger caching
    let entity_id = unique_id("urn:ngsi-ld:Testing:cached-ref");
    let entity = json!({
        "id": &entity_id,
        "type": "Testing",
        "X1": {"type": "Property", "value": "test"},
        "@context": [&hosted_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Verify the referenced URI is now cached
    let (status, cached) = list_contexts(&c, "kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        cached.iter().any(|value| value.as_str() == Some(ctx_url.as_str())),
        "referenced URI should be cached"
    );

    delete_entity(&c, &entity_id).await;
    server.shutdown().await;
}

/// Additional: Add @context with a single string URI -> 201.
#[tokio::test]
#[serial]
async fn add_context_single_string_uri_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let body = json!({
        "@context": "http://smartdatamodels.org/context.jsonld"
    });
    let (status, location, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some(), "should have Location header");
}
