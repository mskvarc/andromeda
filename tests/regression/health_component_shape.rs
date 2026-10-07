//! The health endpoint keeps the documented shape of its component details through its typed model.
//!
//! `GET /health` is a broker extension, not a resource of ETSI GS CIM 009 v1.9.1, rendered as a plain `application/json` document that, as clause 6.3.6 does for a body carrying no `@context`, invents none. The shape is a `HealthCheck` with `id`, `type` and a `healthCheck` object whose `status` is `up` or `degraded` and whose `components` each carry a `status` and a `details` object. The PostgreSQL indicator documents the numeric `poolSize`, `idleConnections` and `pingLatencyMs` details and nothing else.
//!
//! Needs the host broker: the case drives the HTTP API.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::collections::BTreeSet;

/// The members a `HealthCheck` document carries and no others.
const DOCUMENT_MEMBERS: [&str; 3] = ["id", "type", "healthCheck"];

/// The details the PostgreSQL indicator documents and no others.
const POSTGRESQL_DETAILS: [&str; 3] = ["idleConnections", "pingLatencyMs", "poolSize"];

/// The member names of a JSON object, sorted.
fn member_names(value: &Value) -> BTreeSet<&str> {
    value.as_object().expect("an object").keys().map(String::as_str).collect()
}

/// The health document is the typed `HealthCheck`: no member beyond the
/// documented ones, every component carrying a `status` and a `details` object,
/// and the PostgreSQL details keeping their documented names and numeric type.
#[tokio::test]
async fn regression_health_component_details_keep_their_documented_shape() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let response = client.raw(Method::GET, "/ngsi-ld/v1/health").send().await.expect("health request");
    assert_eq!(response.status, StatusCode::OK, "health: {:?}", response.body);
    let body = response.json_value().expect("the health document is JSON");

    assert_eq!(member_names(&body), BTreeSet::from(DOCUMENT_MEMBERS), "{body}");
    assert_eq!(body["type"], "HealthCheck", "{body}");
    let health = &body["healthCheck"];
    assert!(matches!(health["status"].as_str(), Some("up" | "degraded")), "{body}");
    let components = health["components"].as_object().expect("healthCheck.components is an object");
    assert!(components.contains_key("postgresql"), "the storage indicator is reported: {body}");
    for (name, component) in components {
        assert!(component["status"].is_string(), "{name} reports a status: {body}");
        assert!(component["details"].is_object(), "{name} reports a details object: {body}");
    }
    let details = &components["postgresql"]["details"];
    assert_eq!(member_names(details), BTreeSet::from(POSTGRESQL_DETAILS), "{body}");
    for member in POSTGRESQL_DETAILS {
        assert!(details[member].is_number(), "{member} is numeric: {body}");
    }

    scope.finish().await.expect("environment cleanup");
}
