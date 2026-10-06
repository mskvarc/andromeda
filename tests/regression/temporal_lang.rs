//! `lang` on the temporal read bindings.
//!
//! ETSI GS CIM 009 v1.9.1 Tables 6.18.3.2-1 and 6.19.3.1-1 list `lang` among the
//! URL parameters of the temporal query and the temporal retrieval, and clause
//! 4.15 says what it does: the `LanguageProperty` members of the answer are returned as
//! Properties whose `value` is the string of the requested language, naming it
//! in a `lang` member.
//!
//! Before this suite existed the parameter was parsed and forwarded to sources
//! but never applied to a local temporal read, so a client that asked for one
//! language was answered the whole `languageMap` of every instance. Every test
//! carries a control read that names no `lang`, so an assertion can only pass
//! when the two answers genuinely differ.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The three instants the `LanguageProperty` under test was observed at, in order.
const OBSERVATIONS: [&str; 3] = ["2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", "2026-01-03T00:00:00Z"];

/// One Temporal Evolution whose `label` `LanguageProperty` holds three instances,
/// each naming itself in English and in German.
fn evolution(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "label": OBSERVATIONS
            .iter()
            .enumerate()
            .map(|(index, observed)| json!({
                "type": "LanguageProperty",
                "languageMap": {"en": format!("Pump {index}"), "de": format!("Pumpe {index}")},
                "observedAt": observed
            }))
            .collect::<Vec<Value>>(),
        "@context": CORE_CONTEXT
    })
}

/// Create one Temporal Evolution and assert clause 6.20.3.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// One temporal read answered as JSON.
async fn read(client: &TestClient, path: &str) -> (StatusCode, Value) {
    let response = client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .header(
            "Link",
            &format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .send()
        .await
        .expect("temporal read request");
    let body = response.json_value().expect("a temporal read answers with JSON");
    (response.status, body)
}

/// The single-entity retrieval of clause 6.19.3.1 under `selection`.
async fn retrieve(client: &TestClient, id: &str, selection: &str) -> Value {
    let path = format!("/ngsi-ld/v1/temporal/entities/{}?{WINDOW}{selection}", urlencoding::encode(id));
    let (status, body) = read(client, &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// The query of clause 6.18.3.2 under `selection`, answered as the one Entity
/// it selected.
async fn query_one(client: &TestClient, id: &str, selection: &str) -> Value {
    let path = format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&id={}&{WINDOW}{selection}", urlencoding::encode(id));
    let (status, body) = read(client, &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let matches = body.as_array().expect("a temporal query answers a collection");
    assert_eq!(matches.len(), 1, "the query answers the one Entity it selected: {body}");
    matches[0].clone() // clone: the assertions outlive the collection they were read from
}

/// The control: without `lang` every instance of `label` is a `LanguageProperty`
/// holding its whole map.
fn assert_unfiltered(body: &Value) {
    let label = body["label"].as_array().expect("the LanguageProperty history is an array");
    assert_eq!(label.len(), OBSERVATIONS.len(), "{body}");
    for instance in label {
        assert_eq!(instance["type"], json!("LanguageProperty"), "{body}");
        assert!(instance.get("languageMap").is_some(), "{body}");
    }
}

/// Clause 4.15: with `lang=<tag>` every instance of the history is a Property
/// whose `value` is the string of that language and whose `lang` names it, and
/// the instants that place the instances in time are kept.
fn assert_in_language(body: &Value, language: &str, strings: [&str; 3]) {
    let label = body["label"].as_array().expect("the LanguageProperty history is an array");
    assert_eq!(label.len(), OBSERVATIONS.len(), "the history is whole: {body}");
    for ((instance, observed), expected) in label.iter().zip(OBSERVATIONS).zip(strings) {
        assert_eq!(instance["type"], json!("Property"), "{body}");
        assert_eq!(instance["value"], json!(expected), "{body}");
        assert_eq!(instance["lang"], json!(language), "{body}");
        assert_eq!(instance["observedAt"], json!(observed), "{body}");
        assert!(instance.get("languageMap").is_none(), "{body}");
    }
}

/// Table 6.19.3.1-1: the retrieval applies `lang` to every instance.
#[tokio::test]
async fn regression_temporal_lang_retrieval_answers_every_instance_in_the_requested_language() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-lang-retrieve");
    create(client, &evolution(&id)).await;

    assert_unfiltered(&retrieve(client, &id, "").await);
    assert_in_language(&retrieve(client, &id, "&lang=en").await, "en", ["Pump 0", "Pump 1", "Pump 2"]);
    assert_in_language(&retrieve(client, &id, "&lang=de").await, "de", ["Pumpe 0", "Pumpe 1", "Pumpe 2"]);

    scope.finish().await.expect("cleanup failed");
}

/// Table 6.18.3.2-1: the query applies `lang` to every instance of every
/// Entity it answers.
#[tokio::test]
async fn regression_temporal_lang_query_answers_every_instance_in_the_requested_language() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-lang-query");
    create(client, &evolution(&id)).await;

    assert_unfiltered(&query_one(client, &id, "").await);
    assert_in_language(&query_one(client, &id, "&lang=en").await, "en", ["Pump 0", "Pump 1", "Pump 2"]);

    scope.finish().await.expect("cleanup failed");
}
