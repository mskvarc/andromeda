//! Context-source filter integration tests.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServerError, MockResponse},
    context_source_server::ContextSourceServer,
    jsonld_context_server::JsonLdContextServer,
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn csf_entity_query_forwards_only_to_a_matching_registration() {
    let environment = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = environment.client().clone(); // clone: the client remains usable while the environment retains cleanup ownership.
    let config = MockServerConfig::from_environment().expect("valid callback configuration");
    let alpha_source = ContextSourceServer::start(config.clone()).await.expect("alpha context source must start"); // clone: each independently owned server needs the same bind configuration.
    let beta_source = ContextSourceServer::start(config.clone()).await.expect("beta context source must start"); // clone: the context server still consumes the original configuration below.
    let context_server = JsonLdContextServer::start(config).await.expect("context server must start");
    let context_path = CallbackPath::parse(format!("/csf-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let context_url = context_server
        .serve(
            context_path,
            Bytes::from_static(
                br#"{"@context":["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",{"sourceGroup":"https://example.org/sourceGroup","Device":"https://example.org/Device"}]}"#,
            ),
            HeaderMap::new(),
        )
        .await
        .expect("context route must be configured");
    let forwarded_path = CallbackPath::parse("/ngsi-ld/v1/entities").expect("absolute forwarding path");
    let mut response_headers = HeaderMap::new();
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
    let remote_id = format!("urn:ngsi-ld:Device:csf-alpha:{}", Uuid::new_v4());
    let response_body = json!([{"id": remote_id, "type": "Device", "@context": [context_url]}]);
    for source in [&alpha_source, &beta_source] {
        source
            .enqueue(
                Method::GET,
                forwarded_path.clone(), // clone: both mock servers own their route key.
                MockResponse::builder()
                    .status(StatusCode::OK)
                    .headers(response_headers.clone()) // clone: both queued responses own identical headers.
                    .body(Bytes::from(serde_json::to_vec(&response_body).expect("static mock response serializes")))
                    .build(),
            )
            .await;
    }
    let root = CallbackPath::parse("/").expect("absolute root path");
    let alpha_endpoint = alpha_source.endpoint(&root).expect("alpha endpoint must be valid");
    let beta_endpoint = beta_source.endpoint(&root).expect("beta endpoint must be valid");
    let mut alpha_registration_id = None;
    for (group, endpoint) in [("alpha", alpha_endpoint), ("beta", beta_endpoint)] {
        let registration_id = format!("urn:ngsi-ld:ContextSourceRegistration:csf-{group}:{}", Uuid::new_v4());
        let registration = json!({
            "id": registration_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Device"}]}],
            "endpoint": endpoint,
            "operations": ["queryEntity"],
            "sourceGroup": {"type": "Property", "value": group},
            "@context": [context_url]
        });
        let created = client.create_registration(&registration).await.expect("registration creation failed");
        assert_eq!(created.status, StatusCode::CREATED);
        if group == "alpha" {
            alpha_registration_id = Some(registration_id);
        }
    }

    let alpha_registration_id = alpha_registration_id.expect("alpha registration identifier must be recorded");
    let retrieved = client
        .retrieve_registration(&alpha_registration_id, Some(context_url.as_str()))
        .await
        .expect("registration retrieval failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let retrieved_body: Value = retrieved.json().expect("registration retrieval must return JSON");
    assert_eq!(retrieved_body["sourceGroup"]["value"], "alpha");

    let query = client
        .query_entities("type=Device&csf=sourceGroup==alpha", "application/ld+json", Some(context_url.as_str()))
        .await
        .expect("CSF entity query failed");
    assert_eq!(query.status, StatusCode::OK);
    let query_body: Value = query.json().expect("CSF query response must be valid JSON");
    assert!(
        query_body
            .as_array()
            .is_some_and(|entities| entities.iter().any(|entity| entity["id"] == remote_id)),
        "matching context source entity must be present: {query_body}"
    );

    let alpha_request = alpha_source
        .next_request(Duration::from_secs(5))
        .await
        .expect("matching context source must receive the query");
    assert_eq!(alpha_request.uri.path(), "/ngsi-ld/v1/entities");
    let beta_result = beta_source.next_request(Duration::from_millis(300)).await;
    assert!(matches!(beta_result, Err(CallbackServerError::RequestTimeout(_))));

    alpha_source.shutdown().await.expect("alpha context source shutdown failed");
    beta_source.shutdown().await.expect("beta context source shutdown failed");
    context_server.shutdown().await.expect("context server shutdown failed");
    environment.reset_and_finish().await.expect("post-test reset failed");
}

/// Clause 4.9: a `csf` trailing path is evaluated "as a `MemberExpression` of
/// ECMA 262" against a Context Source Property's value, the raw JSON the
/// registrant wrote, so `coverage[region]` reads its literal `region` key and
/// the query forwards only to the registration whose region matches.
#[tokio::test]
async fn csf_entity_query_reads_a_compound_context_source_property_by_its_written_key() {
    let environment = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = environment.client().clone(); // clone: the client remains usable while the environment retains cleanup ownership.
    let config = MockServerConfig::from_environment().expect("valid callback configuration");
    let alpha_source = ContextSourceServer::start(config.clone()).await.expect("alpha context source must start"); // clone: each independently owned server needs the same bind configuration.
    let beta_source = ContextSourceServer::start(config.clone()).await.expect("beta context source must start"); // clone: the context server still consumes the original configuration below.
    let context_server = JsonLdContextServer::start(config).await.expect("context server must start");
    let context_path = CallbackPath::parse(format!("/csf-member-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let context_url = context_server
        .serve(
            context_path,
            Bytes::from_static(
                br#"{"@context":["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",{"coverage":"https://example.org/coverage","Device":"https://example.org/Device"}]}"#,
            ),
            HeaderMap::new(),
        )
        .await
        .expect("context route must be configured");
    let forwarded_path = CallbackPath::parse("/ngsi-ld/v1/entities").expect("absolute forwarding path");
    let mut response_headers = HeaderMap::new();
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
    let remote_id = format!("urn:ngsi-ld:Device:csf-member-north:{}", Uuid::new_v4());
    let response_body = json!([{"id": remote_id, "type": "Device", "@context": [context_url]}]);
    for source in [&alpha_source, &beta_source] {
        source
            .enqueue(
                Method::GET,
                forwarded_path.clone(), // clone: both mock servers own their route key.
                MockResponse::builder()
                    .status(StatusCode::OK)
                    .headers(response_headers.clone()) // clone: both queued responses own identical headers.
                    .body(Bytes::from(serde_json::to_vec(&response_body).expect("static mock response serializes")))
                    .build(),
            )
            .await;
    }
    let root = CallbackPath::parse("/").expect("absolute root path");
    let alpha_endpoint = alpha_source.endpoint(&root).expect("alpha endpoint must be valid");
    let beta_endpoint = beta_source.endpoint(&root).expect("beta endpoint must be valid");
    let mut alpha_registration_id = None;
    for (group, endpoint) in [("north", alpha_endpoint), ("south", beta_endpoint)] {
        let registration_id = format!("urn:ngsi-ld:ContextSourceRegistration:csf-member-{group}:{}", Uuid::new_v4());
        let registration = json!({
            "id": registration_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Device"}]}],
            "endpoint": endpoint,
            "operations": ["queryEntity"],
            "coverage": {"type": "Property", "value": {"region": group}},
            "@context": [context_url]
        });
        let created = client.create_registration(&registration).await.expect("registration creation failed");
        assert_eq!(created.status, StatusCode::CREATED);
        if group == "north" {
            alpha_registration_id = Some(registration_id);
        }
    }

    let alpha_registration_id = alpha_registration_id.expect("alpha registration identifier must be recorded");
    let retrieved = client
        .retrieve_registration(&alpha_registration_id, Some(context_url.as_str()))
        .await
        .expect("registration retrieval failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let retrieved_body: Value = retrieved.json().expect("registration retrieval must return JSON");
    assert_eq!(retrieved_body["coverage"]["value"]["region"], "north");

    let query = client
        .query_entities(
            &format!("type=Device&csf={}", urlencoding::encode("coverage[region]==\"north\"")),
            "application/ld+json",
            Some(context_url.as_str()),
        )
        .await
        .expect("CSF entity query failed");
    assert_eq!(query.status, StatusCode::OK);
    let query_body: Value = query.json().expect("CSF query response must be valid JSON");
    assert!(
        query_body
            .as_array()
            .is_some_and(|entities| entities.iter().any(|entity| entity["id"] == remote_id)),
        "matching context source entity must be present: {query_body}"
    );

    let alpha_request = alpha_source
        .next_request(Duration::from_secs(5))
        .await
        .expect("matching context source must receive the query");
    assert_eq!(alpha_request.uri.path(), "/ngsi-ld/v1/entities");
    let beta_result = beta_source.next_request(Duration::from_millis(300)).await;
    assert!(matches!(beta_result, Err(CallbackServerError::RequestTimeout(_))));

    alpha_source.shutdown().await.expect("alpha context source shutdown failed");
    beta_source.shutdown().await.expect("beta context source shutdown failed");
    context_server.shutdown().await.expect("context server shutdown failed");
    environment.reset_and_finish().await.expect("post-test reset failed");
}
