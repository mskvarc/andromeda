//! A Temporal write holding the NGSI-LD Null under a key of a Property value.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4 excepts "the representation of deleted
//! Properties, Relationships or Language Properties in [...] the temporal
//! evolution" from its second NGSI-LD Null prohibition, and nothing but a
//! merge fragment from its third: the Null as "the value of a key value pair
//! within a JSON object, which is the right-hand side of the value of a
//! Property". A Temporal create, add attributes or instance modification
//! therefore refuses the Null under a key, in a structured value, a JSON literal
//! and a `JsonProperty` alike.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The NGSI-LD Null of clause 4.5.0.
const NULL: &str = "urn:ngsi-ld:null";

/// The problem type clause 5.5.4 gives the Null where a write does not admit
/// it.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The expanded name of the `settings` term the request context coerces to
/// `@json`.
const SETTINGS: &str = "https://example.org/settings";

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The instant every observation of this module is made at.
const OBSERVED_AT: &str = "2024-01-01T00:00:00Z";

/// The request context: `settings` coerced to `@json`, then the core context.
fn context() -> Value {
    json!([{"settings": {"@id": SETTINGS, "@type": "@json"}}, CORE_CONTEXT])
}

/// `attribute` as one observation made at [`OBSERVED_AT`].
fn observed(attribute: &Value) -> Value {
    let mut observed = attribute.clone(); // clone: the caller keeps the attribute to name the case
    observed["observedAt"] = json!(OBSERVED_AT);
    observed
}

/// A Temporal Evolution of `id` observing each of `config` once.
fn evolution(id: &str, config: &[Value]) -> Value {
    json!({"id": id, "type": "LiteralProbe", "config": config.iter().map(observed).collect::<Vec<_>>(), "@context": context()})
}

/// Every shape of a Property value holding the Null under a key, by name.
fn null_keyed_values() -> [(&'static str, Value); 4] {
    [
        ("structured value", json!({"type": "Property", "value": {"street": "x", "country": NULL}})),
        ("@json-coerced key", json!({"type": "Property", "value": {"settings": {"a": 1, "b": NULL}}})),
        (
            "@json value",
            json!({"type": "Property", "value": {"@type": "@json", "@value": {"a": 1, "b": NULL}}}),
        ),
        ("JsonProperty", json!({"type": "JsonProperty", "json": {"a": 1, "b": NULL}})),
    ]
}

/// Send `body` as `application/ld+json` to the broker path `path`.
async fn send(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, &format!("/ngsi-ld/v1/{path}"))
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("temporal write request")
}

/// The Temporal Evolution of `id`.
async fn temporal(client: &TestClient, id: &str) -> TestResponse {
    client
        .get_temporal_entity(id, WINDOW, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request")
}

/// Assert `response` is the clause 5.5.4 refusal of the Null under a key of
/// `config`.
fn assert_refused(response: &TestResponse, case: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{case}: {:?}", response.body);
    let problem = response.json_value().expect("a problem detail");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{case}: {problem}");
    let detail = problem["detail"].as_str().expect("a detail");
    assert!(detail.contains("config") && detail.contains(NULL), "{case}: {detail}");
}

/// Clause 5.6.11: a Temporal create of every shape is refused and records
/// nothing.
#[tokio::test]
async fn regression_r03_a_temporal_create_refuses_a_null_under_a_key() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (case, config) in null_keyed_values() {
        let id = scope.id(&format!("LiteralProbe:temporal-{}", case.replace([' ', '@'], "-")));
        let created = send(client, Method::POST, "temporal/entities", &evolution(&id, &[config])).await;
        assert_refused(&created, case);
        let history = temporal(client, &id).await;
        assert_eq!(history.status, StatusCode::NOT_FOUND, "{case}: nothing is recorded: {:?}", history.body);
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.12 and 5.6.14: adding an observation, and replacing a recorded
/// one, with the Null under a key is refused, and the evolution keeps the one
/// instance it held.
#[tokio::test]
async fn regression_r03_adding_or_modifying_a_temporal_instance_refuses_a_null_under_a_key() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("LiteralProbe:temporal-existing");
    let held = json!({"type": "Property", "value": {"settings": {"a": 1}}});
    let created = send(client, Method::POST, "temporal/entities", &evolution(&id, &[held])).await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let encoded = urlencoding::encode(&id);
    let instance_of = |history: &Value| history["config"][0]["instanceId"].as_str().expect("the instance identifier").to_owned();
    let history = temporal(client, &id).await.json_value().expect("a temporal answer");
    let instance = instance_of(&history);

    for (case, config) in null_keyed_values() {
        let added = send(
            client,
            Method::POST,
            &format!("temporal/entities/{encoded}/attrs"),
            &json!({"config": [observed(&config)], "@context": context()}),
        )
        .await;
        assert_refused(&added, &format!("add attributes {case}"));
        let modified = send(
            client,
            Method::PATCH,
            &format!("temporal/entities/{encoded}/attrs/config/{}", urlencoding::encode(&instance)),
            &json!({"config": [config], "@context": context()}),
        )
        .await;
        assert_refused(&modified, &format!("modify attribute instance {case}"));
    }

    let history = temporal(client, &id).await.json_value().expect("a temporal answer");
    assert_eq!(history["config"].as_array().map(Vec::len), Some(1), "{history}");
    assert_eq!(history["config"][0]["value"][SETTINGS]["@value"], json!({"a": 1}), "{history}");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.5.4 excepts "the representation of deleted Properties [...] in
/// [...] the temporal evolution": an instance whose value is the Null and
/// which carries `deletedAt` (clause 4.5.7) is still admitted beside one with
/// a value.
#[tokio::test]
async fn regression_r03_a_temporal_create_still_records_a_deleted_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("LiteralProbe:temporal-deleted");
    let mut deleted = observed(&json!({"type": "Property", "value": NULL}));
    deleted["observedAt"] = json!("2024-01-02T00:00:00Z");
    deleted["deletedAt"] = json!("2024-01-02T00:00:00Z");
    let body = json!({
        "id": id,
        "type": "LiteralProbe",
        "config": [observed(&json!({"type": "Property", "value": 1})), deleted],
        "@context": context()
    });

    let created = send(client, Method::POST, "temporal/entities", &body).await;

    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    scope.finish().await.expect("environment cleanup");
}
