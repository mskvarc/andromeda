#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Distributed temporal entity retrieve tests.
//!
//! Tests distributed operations on single temporal entity retrieval:
//! local-only retrieval, attribute merging from context sources, error handling.

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, environment::TestEnvironment};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use urlencoding::encode;

use crate::context_source_adapter::ContextSourceAdapter as MockServer;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn client() -> Client {
    Client::new()
}

fn jsonld_link_value() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

fn temporal_vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
            {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"}
        ],
        "fuelLevel": [
            {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
            {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"}
        ],
        "@context": TEST_SUITE_CONTEXT
    })
}

fn vehicle_registration(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
    let mut reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {"id": entity_id, "type": "Vehicle"}
                ]
            }
        ],
        "endpoint": endpoint,
        "operations": ["retrieveEntity", "queryEntity", "retrieveTemporal", "queryTemporal"],
        "@context": TEST_SUITE_CONTEXT
    });
    if let Some(m) = mode {
        reg["mode"] = json!(m);
    }
    reg
}

async fn create_temporal_entity(client: &Client, entity: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_temporal_entity request failed");
    resp.status()
}

async fn create_registration(client: &Client, reg: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(reg)
        .send()
        .await
        .expect("create_registration request failed");
    resp.status()
}

async fn get_temporal_entity(client: &Client, id: &str, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities/{}{sep}{query}", encode(id)))
        .header("Accept", "application/json")
        .header("Link", link)
        .send()
        .await
        .expect("get_temporal_entity request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn delete_temporal_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/temporal/entities/{}", encode(id))).send().await;
}

async fn delete_registration(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/csourceRegistrations/{}", encode(id))).send().await;
}

fn warning_header_values(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// local=true retrieves only local temporal entity, skips CSR forwarding.
/// Spec ref: 5.7.3.4 - when local=true, distributed operations are not performed.
#[tokio::test]
async fn distop_temporal_retrieve_local_true_skips_forwarding() {
    let scope = TestEnvironment::from_suite("distop_temporal_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:tr-local");
    let reg_id = scope.id("Registration:tr-local");

    // Create local temporal entity
    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await, StatusCode::CREATED);

    // Create CSR
    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Stub retrieve endpoint
    let mock_entity = temporal_vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve with local=true
    let (status, _, _) = get_temporal_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Mock should not be called
    pretty_assertions::assert_eq!(
        mock.get_count(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}")),
        0,
        "mock should not be called when local=true"
    );

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve merges local and remote temporal entity attributes.
/// Spec ref: 5.7.3.4 - inclusive mode merges attributes from context sources.
#[tokio::test]
async fn distop_temporal_retrieve_merges_local_and_remote_attributes() {
    let scope = TestEnvironment::from_suite("distop_temporal_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:tr-merge");
    let reg_id = scope.id("Registration:tr-merge");

    // Create local temporal entity (has speed, fuelLevel)
    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await, StatusCode::CREATED);

    // Create CSR
    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Mock returns entity with extra attribute (altitude)
    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "altitude": [{"type": "Property", "value": 500, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, body) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Should have both local (speed, fuelLevel) and remote (altitude) attributes
    assert!(body.get("speed").is_some(), "local attribute 'speed' should be present");
    assert!(body.get("altitude").is_some(), "remote attribute 'altitude' should be merged");

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve returns entity only from remote context source when no local entity exists.
/// Spec ref: 5.7.3.4 - when entity exists only in context source, return from there.
#[tokio::test]
async fn distop_temporal_retrieve_remote_only_entity() {
    let scope = TestEnvironment::from_suite("distop_temporal_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:tr-remote-only");
    let reg_id = scope.id("Registration:tr-remote-only");

    // No local entity - only exists in remote
    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 42, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, body) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(body.get("id").and_then(|v| v.as_str()), Some(entity_id.as_str()));
    assert!(body.get("speed").is_some(), "remote entity speed should be present");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve returns NGSILD-Warning header when context source returns error.
/// Spec ref: 5.7.3.4 - errors from context sources result in 299 warning.
#[tokio::test]
async fn distop_temporal_retrieve_warning_on_context_source_error() {
    let scope = TestEnvironment::from_suite("distop_temporal_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:tr-warn");
    let reg_id = scope.id("Registration:tr-warn");

    // Create local temporal entity so we get 200 even with CS error
    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Mock returns 500 error
    mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 500).await;

    let (status, headers, _) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header on CS error, got none");
    assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve with an exclusive CSR takes the registered Attribute from the
/// context source while an unregistered Attribute stays local.
///
/// Clause 4.3.6.2 requires an exclusive registration to define "both: An entity
/// id [...] Attributes", and clause 5.9.2.4 raises `Conflict` "If an Entity
/// already exists for the supplied Entity ID (URI) and the existing Entity
/// contains any of the Attributes defined in the registration". The local
/// Temporal Evolution therefore holds only `fuelLevel`, which clause 4.3.6.3
/// permits ("Unregistered Attributes of an Entity are permitted to be held
/// locally"), while `speed` is registered and comes from the source. Clause
/// 5.7.3.4 then merges the forwarded result of an exclusive source into S1.
#[tokio::test]
async fn distop_temporal_retrieve_exclusive_mode_returns_remote() {
    let scope = TestEnvironment::from_suite("distop_temporal_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:tr-exclusive");
    let reg_id = scope.id("Registration:tr-exclusive");

    // Local temporal entity holds only the attribute the registration leaves out
    let local_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "fuelLevel": [{"type": "Property", "value": 60, "observedAt": "2020-08-01T12:00:00Z"}],
        "@context": TEST_SUITE_CONTEXT
    });
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &local_entity).await, StatusCode::CREATED);

    // Create CSR in exclusive mode over the entity id and the `speed` property
    let mut reg = vehicle_registration(&reg_id, &entity_id, &mock.url(""), Some("exclusive"));
    reg["information"][0]["propertyNames"] = json!(["speed"]);
    let reg = reg;
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await, StatusCode::CREATED);

    // Mock returns the registered attribute
    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 42, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, body) = get_temporal_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // The registered attribute comes from the exclusive source
    let speed = body.get("speed").expect("speed should be present");
    let speed_values: Vec<i64> = speed.as_array().unwrap().iter().filter_map(|v| v["value"].as_i64()).collect();
    assert!(
        speed_values.contains(&42),
        "exclusive mode should return remote speed=42, got: {speed_values:?}"
    );
    assert!(body.get("fuelLevel").is_some(), "the unregistered local attribute stays with the broker");

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
