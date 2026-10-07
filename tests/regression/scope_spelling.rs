//! A Scope is returned as it was written, and matched by its levels.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.18 makes the leading `/` of a Scope
//! optional -- `Scope = [%x2F] ScopeLevel *(%x2F ScopeLevel)`, with `Madrid`
//! as its EXAMPLE 2 -- and clause 4.6.4 has a broker "return the original
//! content when replying to context consumption requests". The Temporal
//! Evolution and the Context Source Registration store a Scope as a label path,
//! so they must not rebuild it with a leading `/`: `CompanyA` stays `CompanyA`.
//! Core entities are read from their stored document and keep the spelling.
//!
//! Clause 4.19 spells every `scopeQ` pattern with a leading `/`, so a Scope
//! written without one is reachable only when matching compares levels: the
//! last three tests pin that a rooted pattern selects a bare Scope on the Core,
//! temporal and registration paths alike.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The instant the reified Scope of every evolution here is observed at.
const SCOPED_AT: &str = "2018-08-01T11:00:00Z";

/// A temporal window that opens before [`SCOPED_AT`] and never closes.
const WINDOW: &str = "timerel=after&timeAt=2018-01-01T00:00:00Z";

/// A scope level unique to this test environment, so no other test's entity
/// or registration can match a pattern built from it.
fn unique_level(environment: &TestEnvironment) -> String {
    format!("T{}", environment.id_prefix().chars().filter(char::is_ascii_alphanumeric).collect::<String>())
}

/// A Temporal Evolution of a `Vehicle` whose one reified Scope instance holds
/// `paths`, with one `speed` instance beside it.
fn scoped_evolution(id: &str, paths: &Value) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "scope": {"type": "Property", "value": paths, "observedAt": SCOPED_AT},
        "speed": [{"type": "Property", "value": 10, "observedAt": "2018-08-01T11:05:00Z"}],
        "@context": CORE_CONTEXT
    })
}

/// A registration of `Vehicle` entities in `scopes`.
fn scoped_registration(id: &str, scopes: &Value) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "scope": scopes,
        "endpoint": "http://my.csource.org:1026/",
        "@context": [CORE_CONTEXT]
    })
}

/// Create one Temporal Evolution and assert clause 6.20.3.1 answered with 201.
async fn create_evolution(client: &TestClient, body: &Value) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// Create one registration and assert the broker stored it.
async fn create_registration(client: &TestClient, body: &Value) {
    let response = client.create_registration(body).await.expect("create registration request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// The identifiers a list response holds.
fn ids_of(body: &Value) -> Vec<&str> {
    body.as_array()
        .expect("a list response is an array")
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect()
}

/// The `value` of each reified Scope instance a normalized Temporal Evolution
/// carries.
fn reified_scope_values(evolution: &Value) -> Vec<&Value> {
    match &evolution["scope"] {
        Value::Array(instances) => instances.iter().map(|instance| &instance["value"]).collect(),
        instance @ Value::Object(_) => vec![&instance["value"]],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    }
}

/// Clause 4.18 admits a Scope without its leading `/`, and clause 4.6.4 has it
/// returned as written: the reified Scope of a Temporal Evolution keeps both
/// spellings it was given.
#[tokio::test]
async fn regression_scope_spelling_temporal_evolution_returns_each_scope_as_written() {
    let environment = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = environment.client();
    let level = unique_level(&environment);
    let id = environment.id("Vehicle:scope-spelling-temporal");
    let written = json!([format!("/{level}/Gardens"), level]);

    create_evolution(client, &scoped_evolution(&id, &written)).await;

    let response = client
        .get_temporal_entity(&id, "", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve: {:?}", response.body);
    let evolution = response.json_value().expect("the temporal retrieval answers with JSON");
    assert_eq!(
        reified_scope_values(&evolution),
        vec![&written],
        "each Scope reads back as written: {evolution}"
    );

    environment.finish().await.expect("cleanup failed");
}

/// Table 5.2.9-1 `scope` takes clause 4.18 Scopes, so a registration's Scopes
/// read back as written too.
#[tokio::test]
async fn regression_scope_spelling_registration_returns_each_scope_as_written() {
    let environment = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = environment.client();
    let level = unique_level(&environment);
    let id = environment.id("ContextSourceRegistration:scope-spelling-written");
    let written = json!([level, format!("/{level}/Gardens")]);

    create_registration(client, &scoped_registration(&id, &written)).await;

    let response = client
        .retrieve_registration(&id, Some(CORE_CONTEXT))
        .await
        .expect("retrieve registration request");
    assert_eq!(response.status, StatusCode::OK, "retrieve: {:?}", response.body);
    let registration = response.json_value().expect("a retrieved registration is JSON");
    assert_eq!(registration["scope"], written, "each Scope reads back as written: {registration}");

    environment.finish().await.expect("cleanup failed");
}

/// Clause 4.19 spells a pattern with its leading `/`, so a rooted pattern has
/// to select an entity whose Scope was written without one; the entity still
/// reads back with the spelling it was given.
#[tokio::test]
async fn regression_scope_spelling_rooted_scope_query_selects_a_bare_entity_scope() {
    let environment = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = environment.client();
    let level = unique_level(&environment);
    let id = environment.id("Building:scope-spelling-core");

    let entity = json!({"id": id, "type": "Building", "scope": level, "@context": [CORE_CONTEXT]});
    let created = client.post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let query = format!(
        "type=Building&id={}&scopeQ={}",
        urlencoding::encode(&id),
        urlencoding::encode(&format!("/{level}"))
    );
    let response = client
        .query_entities(&query, "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("query entities request");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);
    let body = response.json_value().expect("the query answers with JSON");
    assert_eq!(ids_of(&body), vec![id.as_str()], "/{level} selects the entity scoped {level}: {body}");
    assert_eq!(body[0]["scope"], json!(level), "the Scope reads back as written: {body}");

    environment.finish().await.expect("cleanup failed");
}

/// The temporal `scopeQ` reads the recorded Scopes by the same level rule as
/// the Core one.
#[tokio::test]
async fn regression_scope_spelling_rooted_temporal_scope_query_selects_a_bare_scope() {
    let environment = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = environment.client();
    let level = unique_level(&environment);
    let id = environment.id("Vehicle:scope-spelling-temporal-query");

    create_evolution(client, &scoped_evolution(&id, &json!(level))).await;

    let query = format!(
        "type=Vehicle&id={}&scopeQ={}&{WINDOW}",
        urlencoding::encode(&id),
        urlencoding::encode(&format!("/{level}"))
    );
    let response = client
        .query_temporal_entities(&query, Some(CORE_CONTEXT))
        .await
        .expect("query temporal entities request");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);
    let body = response.json_value().expect("the temporal query answers with JSON");
    assert_eq!(ids_of(&body), vec![id.as_str()], "/{level} selects the evolution scoped {level}: {body}");

    environment.finish().await.expect("cleanup failed");
}

/// A registration's `scope` is matched by the same level rule.
#[tokio::test]
async fn regression_scope_spelling_rooted_scope_query_selects_a_bare_registration_scope() {
    let environment = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = environment.client();
    let level = unique_level(&environment);
    let id = environment.id("ContextSourceRegistration:scope-spelling-query");

    create_registration(client, &scoped_registration(&id, &json!([level]))).await;

    let query = format!("type=Vehicle&scopeQ={}", urlencoding::encode(&format!("/{level}")));
    let response = client
        .query_registrations(&query, Some(CORE_CONTEXT))
        .await
        .expect("query registrations request");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);
    let body = response.json_value().expect("the discovery answers with JSON");
    assert_eq!(ids_of(&body), vec![id.as_str()], "/{level} selects the registration scoped {level}: {body}");

    environment.finish().await.expect("cleanup failed");
}
