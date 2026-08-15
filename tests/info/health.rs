//! Broker-health integration tests.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::CONTENT_TYPE};
use serde_json::Value;
use std::collections::BTreeSet;

/// A healthy assembled broker reports its aggregate status and concrete indicators.
#[tokio::test]
async fn info_health_running_broker_returns_active_component_statuses() {
    let scope = TestEnvironment::from_suite("info-health").await.expect("isolated test environment");
    let client = scope.client();
    let response = client.raw(Method::GET, "/ngsi-ld/v1/health").send().await.expect("health request failed");

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.headers.get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    let body = response.json_value().expect("response body must be valid JSON");
    let id = body["id"].as_str().expect("health id must be a string");
    assert!(id.starts_with("urn:ngsi-ld:HealthCheck:"), "unexpected health id: {id}");
    assert_eq!(body["type"], "HealthCheck");
    assert!(body.get("@context").is_none(), "the broker extension must not invent a JSON-LD context");

    let health = body["healthCheck"].as_object().expect("healthCheck must be an object");
    assert!(matches!(health["status"].as_str(), Some("up" | "degraded")));
    let components = health["components"].as_object().expect("health components must be an object");
    let component_names = components.keys().map(String::as_str).collect::<BTreeSet<_>>();
    assert!(
        component_names.contains("postgresql"),
        "the selected storage profile must register its health indicator"
    );
    assert!(
        component_names.iter().all(|name| matches!(*name, "mqtt" | "postgresql")),
        "health must expose only active known indicators"
    );

    let postgresql = components["postgresql"].as_object().expect("postgresql health must be an object");
    assert_eq!(postgresql["status"], "up");
    let details = postgresql["details"].as_object().expect("postgresql details must be an object");
    for member in ["poolSize", "idleConnections", "pingLatencyMs"] {
        assert!(details.get(member).is_some_and(Value::is_number), "{member} must be numeric");
    }
}
