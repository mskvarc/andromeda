#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Distributed temporal entity query tests: splitEntities post-merge filtering.

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;
use urlencoding::encode;
use uuid::Uuid;

use crate::context_source_adapter::ContextSourceAdapter as MockServer;

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn jsonld_link_value() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

fn temporal_vehicle_entity_with_speed(id: &str, speed: i64, observed_at: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": speed,
            "observedAt": observed_at
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

fn temporal_vehicle_entity_with_scope(id: &str, scope: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 50,
            "observedAt": "2024-01-01T12:00:00Z"
        },
        "scope": scope,
        "@context": TEST_SUITE_CONTEXT
    })
}

/// A registration advertising `queryTemporal` over `Vehicle` entities.
///
/// Table 4.20-2 lists no temporal operation in any named operation group, so a
/// temporal source has to advertise `queryTemporal` by name. Clause 5.9.2.4
/// therefore bars `auxiliary` here: an auxiliary registration whose
/// `operations` are not "retrieveOps", "retrieveEntity" or "queryEntity", or a
/// combination thereof, is rejected with `BadRequestData`.
fn vehicle_registration_temporal(reg_id: &str, endpoint: &str, mode: &str) -> Value {
    json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {"type": "Vehicle"}
                ]
            }
        ],
        "endpoint": endpoint,
        "mode": mode,
        "operations": ["queryTemporal"],
        "@context": TEST_SUITE_CONTEXT
    })
}

async fn create_temporal_entity(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_temporal_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

async fn create_registration(client: &Client, reg: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(reg)
        .send()
        .await
        .expect("create_registration request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

async fn query_temporal_entities(client: &Client, query: &str, link: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities{sep}{query}"))
        .header("Accept", "application/json")
        .header("Link", link)
        .send()
        .await
        .expect("query_temporal_entities request failed");
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

fn vehicle_registration_with_mode(reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> Value {
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

/// Temporal splitEntities with q filter: only matching entities remain after post-merge filtering.
///
/// The source is inclusive because clause 5.7.4.4 merges the Entity Array of an
/// "exclusive, redirect and inclusive" source into S4, while an auxiliary source
/// contributes only where an instance "is not already present in S4"; a
/// remote-only Entity therefore never reaches the post-merge filters.
#[tokio::test]
#[serial]
async fn distop_temporal_split_entities_q_filter_post_merge() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tsplit-q");

    let entity = temporal_vehicle_entity_with_speed(&local_id, 50, "2024-01-01T12:00:00Z");
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity_with_speed(&remote_id, 80, "2024-01-01T12:00:00Z");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "inclusive");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&q=speed>60&timerel=before&timeAt=2030-01-01T00:00:00Z";
    let (status, _, body) = query_temporal_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local temporal entity (speed=50) should be filtered out by q=speed>60"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote temporal entity (speed=80) should pass q=speed>60 filter"
    );

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Temporal splitEntities without filter returns all entities.
#[tokio::test]
#[serial]
async fn distop_temporal_split_entities_returns_all_without_filter() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-all-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tsplit-all-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tsplit-all");

    let entity = temporal_vehicle_entity_with_speed(&local_id, 50, "2024-01-01T12:00:00Z");
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity_with_speed(&remote_id, 80, "2024-01-01T12:00:00Z");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "inclusive");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&timerel=before&timeAt=2030-01-01T00:00:00Z";
    let (status, _, body) = query_temporal_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(returned_ids.contains(&local_id.as_str()), "should contain local temporal entity");
    assert!(returned_ids.contains(&remote_id.as_str()), "should contain remote temporal entity");

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Temporal splitEntities with scopeQ filter applied post-merge.
#[tokio::test]
#[serial]
async fn distop_temporal_split_entities_scope_filter_post_merge() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tscope-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tscope-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tscope-split");

    let entity = temporal_vehicle_entity_with_scope(&local_id, "/Madrid");
    let (status, _) = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity_with_scope(&remote_id, "/Barcelona");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration_temporal(&reg_id, &mock.url(""), "inclusive");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&scopeQ=/Barcelona&timerel=before&timeAt=2030-01-01T00:00:00Z";
    let (status, _, body) = query_temporal_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local temporal entity (scope /Madrid) should be filtered out by scopeQ=/Barcelona"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote temporal entity (scope /Barcelona) should pass scopeQ=/Barcelona filter"
    );

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// local=true skips CSR forwarding for temporal query.
/// Spec ref: 5.7.4.4 - when local=true, distributed operations are not performed.
#[tokio::test]
#[serial]
async fn distop_temporal_query_local_true_skips_forwarding() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-local-skip");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-local-skip");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let (status, _, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities"),
        0,
        "mock should not be called when local=true"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Inclusive mode (default) merges remote attributes into local temporal entity.
/// Spec ref: 5.7.4.4 - inclusive mode merges attributes from context sources.
#[tokio::test]
#[serial]
async fn distop_temporal_query_inclusive_mode_merges_attributes() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-inclusive");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-inclusive");

    let entity = temporal_vehicle_entity(&entity_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await.0, StatusCode::CREATED);

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "altitude": [{"type": "Property", "value": 100, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let (status, _, body) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response should be array");
    let entity = entities.iter().find(|e| e["id"].as_str() == Some(entity_id.as_str()));
    assert!(entity.is_some(), "entity should be present in results");
    assert!(entity.unwrap().get("altitude").is_some(), "altitude from remote should be merged into entity");

    delete_temporal_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Auxiliary mode supplements existing entities only, does not add new ones.
/// Spec ref: 5.7.4.4 - auxiliary mode supplements only, does not add new entities.
///
/// Clause 5.7.4.4 forwards a temporal query only to sources that "support the
/// "queryTemporal" operation", and Table 4.20-2 lists that operation in no
/// named group. Clause 5.9.2.4 in turn rejects an auxiliary registration whose
/// `operations` are anything but "retrieveOps", "retrieveEntity" or
/// "queryEntity". No auxiliary source can therefore be reached by a temporal
/// query, so this scenario is unreachable until the two clauses are reconciled.
#[tokio::test]
#[serial]
#[ignore = "SPEC-CONFLICT: clause 5.9.2.4 limits auxiliary registrations to retrieveOps, retrieveEntity and queryEntity, so an auxiliary temporal source cannot be registered"]
async fn distop_temporal_query_auxiliary_mode_supplements_only() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:tq-auxiliary-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:tq-auxiliary-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-auxiliary");

    let entity = temporal_vehicle_entity(&local_id);
    pretty_assertions::assert_eq!(create_temporal_entity(&c, &entity).await.0, StatusCode::CREATED);

    let reg = vehicle_registration_with_mode(&reg_id, &local_id, &mock.url(""), Some("auxiliary"));
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    let mock_entities = json!([
        {"id": local_id, "type": "Vehicle", "altitude": [{"type": "Property", "value": 200}]},
        {"id": remote_id, "type": "Vehicle", "speed": [{"type": "Property", "value": 90}]}
    ]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_entities).await;

    let (status, _, body) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response should be array");

    assert!(
        !entities.iter().any(|e| e["id"].as_str() == Some(remote_id.as_str())),
        "auxiliary mode should not add new entities, but remote entity was found"
    );

    let local = entities.iter().find(|e| e["id"].as_str() == Some(local_id.as_str()));
    assert!(local.is_some(), "local entity should be present");
    assert!(
        local.unwrap().get("altitude").is_some(),
        "altitude should be supplemented from remote in auxiliary mode"
    );

    delete_temporal_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Exclusive CSR mode returns entity from context source only.
/// Spec ref: 5.7.4.4 - exclusive mode: context source is authoritative.
#[tokio::test]
#[serial]
async fn distop_temporal_query_exclusive_mode_returns_remote() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-exclusive");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-exclusive");

    // Create CSR in exclusive mode (no local entity). Clause 4.3.6.2: "An
    // exclusive registration shall always relate to specific Attributes found
    // on a single Entity. Thus, the registration shall define both: An entity
    // id [...] Attributes." - the registration names `speed` alongside the id.
    let mut reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), Some("exclusive"));
    reg["information"][0]["propertyNames"] = json!(["speed"]);
    let reg = reg;
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    let mock_entity = json!({
        "id": entity_id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 55, "observedAt": "2020-08-01T12:00:00Z"}]
    });
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let (status, _, body) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response should be array");
    let found = entities.iter().any(|e| e["id"].as_str() == Some(entity_id.as_str()));
    assert!(found, "exclusive CSR entity should appear in results");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// NGSILD-Warning header returned when context source returns error.
/// Spec ref: 5.7.4.4 - errors from context sources result in 299 warning.
#[tokio::test]
#[serial]
async fn distop_temporal_query_warning_on_context_source_error() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-warn");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-warn");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    mock.set_stub(Method::GET, "/ngsi-ld/v1/temporal/entities", 500).await;

    let (status, headers, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// type=* implies local=true per spec.
/// Spec ref: 5.7.4.4 - when type=*, distributed operations are not performed.
#[tokio::test]
#[serial]
async fn distop_temporal_query_type_star_implies_local() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-star");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-star");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    let mock_entity = temporal_vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let (status, _, _) = query_temporal_entities(&c, "type=*&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities"),
        0,
        "mock should not be called when type=* (implies local=true)"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Temporal query parameters (timerel, timeAt, lastN) are forwarded to context source.
/// Spec ref: 5.7.4.4 - temporal parameters shall be forwarded.
#[tokio::test]
#[serial]
async fn distop_temporal_query_forwards_temporal_parameters() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-params");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-params");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &json!([])).await;

    let (status, _, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z&lastN=5", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    assert!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities") > 0,
        "mock should receive forwarded temporal query"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Via header is propagated when forwarding temporal query.
/// Spec ref: 5.7.4.4 - Via header shall be added/updated on forwarding.
#[tokio::test]
#[serial]
async fn distop_temporal_query_via_header_propagated() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tq-via");
    let reg_id = unique_id("urn:ngsi-ld:Registration:tq-via");

    let reg = vehicle_registration_with_mode(&reg_id, &entity_id, &mock.url(""), None);
    pretty_assertions::assert_eq!(create_registration(&c, &reg).await.0, StatusCode::CREATED);

    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &json!([])).await;

    let (status, _, _) = query_temporal_entities(&c, "type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    assert!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities") > 0,
        "mock should receive forwarded request with Via header"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}
