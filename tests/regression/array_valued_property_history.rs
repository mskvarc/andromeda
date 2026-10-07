//! ETSI GS CIM 009 v1.9.1 clause 5.2.5 puts no restriction on a Property `value`: Table 5.2.5-1 gives it the data type "JSON value", so an array and a JSON object are both admissible.
//!
//! A JSON-LD value node holds a string, number, boolean or null and nothing else, so an array-valued Property is stored as the elements of the value nodes JSON-LD expansion produced, and the temporal rendering has to produce those same value nodes back. That is observable: `["a"]` is stored as the string `"a"` and `[["a"],["b","c"]]` as `["a","b","c"]`, because expansion collapses a one-element array and flattens a nested one. Every test asserts the temporal answer against the current representation of the same Attribute, which is what a client comparing the two would see.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The instant every observation in this module carries.
const OBSERVED_AT: &str = "2024-01-01T00:00:00Z";

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// One Temporal Evolution carrying a single observation of one Attribute.
fn evolution(id: &str, name: &str, instance: &Value) -> Value {
    json!({"id": id, "type": "ArrayProbe", name: [instance], "@context": CORE_CONTEXT})
}

/// One Property observation of the given value.
fn property(value: &Value) -> Value {
    json!({"type": "Property", "value": value, "observedAt": OBSERVED_AT})
}

/// Create one Temporal Evolution and assert clause 6.20.3.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// The Temporal Evolution of one Entity, which clause 5.7.3 answers with 200.
async fn temporal(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_temporal_entity(id, query, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieval of {id}: {:?}", response.body);
    response.json_value().expect("a temporal answer is JSON")
}

/// The current representation of one Entity (clause 5.7.1).
async fn current(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_entity(id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "current retrieval of {id}: {:?}", response.body);
    response.json_value().expect("an entity is JSON")
}

/// The one Entity a temporal query returns for `id`.
async fn queried(client: &TestClient, id: &str) -> Value {
    let response = client
        .query_temporal_entities(&format!("type=ArrayProbe&id={id}&{WINDOW}"), Some(CORE_CONTEXT))
        .await
        .expect("temporal query request");
    assert_eq!(response.status, StatusCode::OK, "temporal query for {id}: {:?}", response.body);
    let answer = response.json_value().expect("a temporal query answer is JSON");
    assert!(answer.is_array(), "a query answers a collection: {answer}");
    let matches = answer.as_array().expect("the assertion above holds");
    assert_eq!(matches.len(), 1, "the query answers the one Entity it selected: {answer}");
    matches[0].clone() // clone: the caller owns the record it asserts on
}

/// The single instance the fixture writes is what lets the temporal reading be compared with the current one directly: clause 4.5.7 renders the instance as an array of one, and clause 4.5.9 renders it as one `[value, instant]` pair.
async fn assert_value_round_trips(scope: &TestEnvironment, local: &str, name: &str, written: Value, expected: &Value) {
    let id = scope.id(local);
    create(scope.client(), &evolution(&id, name, &property(&written))).await;

    let retrieved = temporal(scope.client(), &id, WINDOW).await;
    assert_eq!(&retrieved[name][0]["value"], expected, "the temporal retrieval: {retrieved}");
    assert_eq!(retrieved[name][0]["type"], json!("Property"), "{retrieved}");

    let from_query = queried(scope.client(), &id).await;
    assert_eq!(&from_query[name][0]["value"], expected, "the temporal query: {from_query}");

    let simplified = temporal(scope.client(), &id, &format!("{WINDOW}&format=temporalValues")).await;
    assert_eq!(
        &simplified[name]["values"][0][0], expected,
        "clause 4.5.9's [value, instant] pair: {simplified}"
    );
    assert_eq!(simplified[name]["values"][0][1], json!(OBSERVED_AT), "{simplified}");

    let current = current(scope.client(), &id).await;
    assert_eq!(
        &current[name]["value"], expected,
        "the temporal and the current representation answer the same value: {current}"
    );
}

/// Clause 5.2.5: an array of strings is a Property value, and the whole array
/// comes back from every temporal reading.
#[tokio::test]
async fn regression_array_valued_property_history_an_array_of_strings_round_trips() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_value_round_trips(&scope, "ArrayProbe:strings", "tags", json!(["a", "b"]), &json!(["a", "b"])).await;

    scope.finish().await.expect("cleanup failed");
}

/// The same for an array of numbers, whose elements reach a different storage
/// reading than a string's.
#[tokio::test]
async fn regression_array_valued_property_history_an_array_of_numbers_round_trips() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_value_round_trips(&scope, "ArrayProbe:numbers", "readings", json!([1, 2, 3]), &json!([1, 2, 3])).await;

    scope.finish().await.expect("cleanup failed");
}

/// A nested array is flattened by JSON-LD expansion before it is stored, so both representations answer the flattened array — the same answer, which is the property under test.
#[tokio::test]
async fn regression_array_valued_property_history_a_nested_array_reads_back_as_the_expansion_stored_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_value_round_trips(&scope, "ArrayProbe:nested", "grid", json!([["a"], ["b", "c"]]), &json!(["a", "b", "c"])).await;

    scope.finish().await.expect("cleanup failed");
}

/// A one-element array is one value node after expansion, so it is stored and answered as the bare element — identically by both representations.
#[tokio::test]
async fn regression_array_valued_property_history_a_one_element_array_reads_back_as_its_element() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_value_round_trips(&scope, "ArrayProbe:single", "tag", json!(["a"]), &json!("a")).await;

    scope.finish().await.expect("cleanup failed");
}

/// A JSON object is a Property value of the same family: it too could not be carried by one value node, and it too is stored as what expansion made of it.
#[tokio::test]
async fn regression_array_valued_property_history_an_object_valued_property_round_trips() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_value_round_trips(
        &scope,
        "ArrayProbe:object",
        "reading",
        json!({"unit": "km", "amount": 3}),
        &json!({"unit": "km", "amount": 3}),
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 3.1 lists "an array" among the NGSI-LD Values, and the empty array is one: JSON-LD expansion keeps the `value` member with no value node, the Temporal Evolution records it, and every reading answers `[]`.
#[tokio::test]
async fn regression_array_valued_property_history_an_empty_array_round_trips() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_value_round_trips(&scope, "ArrayProbe:empty", "tags", json!([]), &json!([])).await;

    scope.finish().await.expect("cleanup failed");
}

/// An array inside a sub-Attribute is stored with the sub-Attribute rather than in the value column, so it takes a different route to the same rendering and is asserted separately.
#[tokio::test]
async fn regression_array_valued_property_history_an_array_inside_a_sub_attribute_round_trips() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayProbe:sub-attribute");

    let instance = json!({
        "type": "Property",
        "value": 1,
        "observedAt": OBSERVED_AT,
        "sources": {"type": "Property", "value": ["x", "y"]}
    });
    create(scope.client(), &evolution(&id, "speed", &instance)).await;

    let retrieved = temporal(scope.client(), &id, WINDOW).await;
    assert_eq!(retrieved["speed"][0]["value"], json!(1), "{retrieved}");
    assert_eq!(
        retrieved["speed"][0]["sources"],
        json!({"type": "Property", "value": ["x", "y"]}),
        "the sub-Attribute keeps its array: {retrieved}"
    );

    let current = current(scope.client(), &id).await;
    assert_eq!(current["speed"]["sources"], retrieved["speed"][0]["sources"], "{current}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.38's `JsonProperty` carries its array as a JSON literal in `json` rather than as a Property `value`, and that distinction survives both readings — the array-valued Property is not turned into one.
#[tokio::test]
async fn regression_array_valued_property_history_a_json_property_array_stays_a_json_literal() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayProbe:json-property");

    let instance = json!({"type": "JsonProperty", "json": [{"a": 1}, {"b": 2}], "observedAt": OBSERVED_AT});
    create(scope.client(), &evolution(&id, "document", &instance)).await;

    let retrieved = temporal(scope.client(), &id, WINDOW).await;
    assert_eq!(retrieved["document"][0]["type"], json!("JsonProperty"), "{retrieved}");
    assert_eq!(retrieved["document"][0]["json"], json!([{"a": 1}, {"b": 2}]), "{retrieved}");
    assert_eq!(
        retrieved["document"][0]["value"],
        Value::Null,
        "a JsonProperty has no value member: {retrieved}"
    );

    // Clause 4.5.9 wraps a `JsonProperty` value in its own member inside the
    // simplified pair, which is the difference from a Property's bare value.
    let simplified = temporal(scope.client(), &id, &format!("{WINDOW}&format=temporalValues")).await;
    assert_eq!(simplified["document"]["jsons"][0][0], json!({"json": [{"a": 1}, {"b": 2}]}), "{simplified}");

    let current = current(scope.client(), &id).await;
    assert_eq!(current["document"]["json"], json!([{"a": 1}, {"b": 2}]), "{current}");

    scope.finish().await.expect("cleanup failed");
}
