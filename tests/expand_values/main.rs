//! `expandValues` query parameter integration tests (NGSI-LD 6.3.7).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use andromeda_test_mocks::{callback_path::CallbackPath, jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::{body::Bytes, http::HeaderMap};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn expand_values_entity_query_expands_vocab_typed_filter_values() {
    let environment = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = environment.client().clone(); // clone: the client remains usable while the environment retains cleanup ownership.
    let context_server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let context_path = CallbackPath::parse(format!("/expand-values-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let context_url = context_server
        .serve(
            context_path,
            Bytes::from_static(br#"{"@context":{"status":{"@id":"https://example.org/status","@type":"@vocab"},"active":"https://example.org/state/active"}}"#),
            HeaderMap::new(),
        )
        .await
        .expect("context route must be configured");
    let entity_id = format!("urn:ngsi-ld:Device:expand-values:{}", Uuid::new_v4());
    let entity = json!({
        "id": entity_id,
        "type": "Device",
        "status": {"type": "Property", "value": "https://example.org/state/active"},
        "@context": [context_url]
    });

    let created = client.post_entity(&entity).send().await.expect("entity creation failed");
    assert_eq!(created.status, StatusCode::CREATED);

    let context_url = context_url.as_str();
    let id = urlencoding::encode(&entity_id);
    let unexpanded = client
        .query_entities(&format!("id={id}&q=status==active"), "application/json", Some(context_url))
        .await
        .expect("unexpanded query failed");
    assert_eq!(unexpanded.status, StatusCode::OK);
    let unexpanded_body: Value = unexpanded.json().expect("unexpanded query must return JSON");
    assert_eq!(unexpanded_body.as_array().map(Vec::len), Some(0));

    let expanded = client
        .query_entities(&format!("id={id}&q=status==active&expandValues=status"), "application/json", Some(context_url))
        .await
        .expect("expanded query failed");
    assert_eq!(expanded.status, StatusCode::OK);
    let expanded_body: Value = expanded.json().expect("expanded query must return JSON");
    assert_eq!(expanded_body.as_array().map(Vec::len), Some(1));

    context_server.shutdown().await.expect("context server shutdown failed");
    environment.reset_and_finish().await.expect("post-test reset failed");
}
