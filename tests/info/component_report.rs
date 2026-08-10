//! Active-component report integration tests.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::CONTENT_TYPE};
use serde_json::{Map, Value};
use sqlx::Row;
use std::collections::BTreeSet;

fn component<'a>(components: &'a [Value], name: &str) -> &'a Map<String, Value> {
    components
        .iter()
        .find(|candidate| candidate["component"] == name)
        .and_then(Value::as_object)
        .expect("component report must contain the requested component object")
}

fn assert_non_empty_string(component: &Map<String, Value>, member: &str) {
    let value = component.get(member).and_then(Value::as_str).expect("build metadata member must be a string");
    assert!(!value.is_empty(), "{member} must not be empty");
}

/// The extension response keeps its stable envelope and complete Andromeda build metadata.
#[tokio::test]
async fn info_component_report_active_profile_returns_stable_envelope() {
    let scope = TestEnvironment::from_suite("info-component-report-envelope")
        .await
        .expect("isolated test environment");
    let client = scope.client();
    let response = client
        .raw(Method::GET, "/ngsi-ld/v1/version")
        .send()
        .await
        .expect("component report request failed");

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.headers.get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    let body = response.json_value().expect("component report must be valid JSON");
    let id = body["id"].as_str().expect("component report id must be a string");
    assert!(id.starts_with("urn:ngsi-ld:ComponentReportList:"), "unexpected component report id: {id}");
    assert_eq!(body["type"], "ComponentReportList");
    assert!(body.get("@context").is_none(), "the broker extension must not invent a JSON-LD context");

    let components = body["components"].as_array().expect("components must be an array");
    let andromeda = component(components, "andromeda");
    for member in [
        "version",
        "buildTime",
        "branch",
        "commit",
        "rustVersion",
        "rustChannel",
        "cargoVersion",
        "buildOperatingSystem",
        "buildTarget",
    ] {
        assert_non_empty_string(andromeda, member);
    }
    assert!(andromeda.get("gitClean").is_some_and(Value::is_boolean), "gitClean must be a boolean");
    scope.finish().await.expect("cleanup failed");
}

/// Storage entries are derived from the selected PostgreSQL profile and installed extensions.
#[tokio::test]
async fn info_component_report_storage_entries_match_active_database_components() {
    let scope = TestEnvironment::from_suite("info-component-report-storage")
        .await
        .expect("isolated test environment");
    let client = scope.client();
    let response = client
        .raw(Method::GET, "/ngsi-ld/v1/version")
        .send()
        .await
        .expect("component report request failed");
    assert_eq!(response.status, StatusCode::OK);

    let body = response.json_value().expect("component report must be valid JSON");
    let components = body["components"].as_array().expect("components must be an array");
    let component_names = components
        .iter()
        .map(|entry| entry["component"].as_str().expect("component name must be a string"))
        .collect::<BTreeSet<_>>();
    assert_eq!(component_names.len(), components.len(), "component names must not be duplicated");

    let row = sqlx::query(
        "SELECT current_setting('server_version') AS pg_version, \
         (SELECT extversion FROM pg_extension WHERE extname = 'postgis') AS postgis_version, \
         (SELECT extversion FROM pg_extension WHERE extname = 'timescaledb') AS timescaledb_version, \
         current_setting('timescaledb.license', true) AS timescaledb_license",
    )
    .fetch_one(scope.database_pool())
    .await
    .expect("active database component query failed");
    let postgresql_version: String = row.get("pg_version");
    let postgis_version: Option<String> = row.get("postgis_version");
    let timescaledb_version: Option<String> = row.get("timescaledb_version");
    let timescaledb_license: Option<String> = row.get("timescaledb_license");

    let mut expected_names = BTreeSet::from(["andromeda", "postgresql"]);
    assert_eq!(component(components, "postgresql")["version"], format!("v{postgresql_version}"));
    if let Some(version) = postgis_version {
        expected_names.insert("postgis");
        assert_eq!(component(components, "postgis")["version"], format!("v{version}"));
    }
    if let Some(version) = timescaledb_version {
        expected_names.insert("timescaledb");
        let timescaledb = component(components, "timescaledb");
        assert_eq!(timescaledb["version"], format!("v{version}"));
        let expected_license = match timescaledb_license.as_deref() {
            Some("timescale") => "TSL",
            Some("apache") => "Apache-2.0",
            Some(other) => other,
            None => "unknown",
        };
        assert_eq!(timescaledb["license"], expected_license);
    }
    assert_eq!(
        component_names, expected_names,
        "the response must report exactly the active composition components"
    );
    scope.finish().await.expect("cleanup failed");
}
