//! The attributes the attribute nesting tests write, in every shape that
//! nests, and the refusal an attribute nesting too deeply is answered with.
//!
//! A level is a sub-attribute, a JSON object or list of a value, or a container
//! inside a JSON literal; the broker under test admits 32 of them, its default.

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The deepest nesting the broker under test admits.
pub const LIMIT: usize = 32;

/// The problem type an attribute nesting too deeply is refused with.
pub const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The expanded name of the attribute `p` every test nests, under the core
/// context's default vocabulary.
pub const NESTED: &str = "https://uri.etsi.org/ngsi-ld/default-context/p";

/// A chain of `levels` JSON objects ending in a string.
pub fn object_chain(levels: usize) -> Value {
    (0..levels).fold(json!("leaf"), |inner, _| json!({"k": inner}))
}

/// A chain of `levels` arrays ending in a number.
pub fn array_chain(levels: usize) -> Value {
    (0..levels).fold(json!(1), |inner, _| json!([inner]))
}

/// A chain of `levels` sub-attributes, each a Property of a scalar, below a
/// Property of a scalar.
pub fn sub_attribute_chain(levels: usize) -> Value {
    (0..levels).fold(
        json!({"type": "Property", "value": 1}),
        |inner, _| json!({"type": "Property", "value": 1, "s": inner}),
    )
}

/// Each shape that nests, as the attribute nesting `levels` levels.
pub fn shapes(levels: usize) -> [(&'static str, Value); 5] {
    [
        ("object-chain", json!({"type": "Property", "value": object_chain(levels)})),
        ("list-chain", json!({"type": "ListProperty", "valueList": array_chain(levels)})),
        ("json-literal", json!({"type": "JsonProperty", "json": object_chain(levels)})),
        (
            "sub-attribute-value",
            json!({"type": "Property", "value": 1, "s": {"type": "Property", "value": object_chain(levels - 1)}}),
        ),
        ("sub-attribute-chain", sub_attribute_chain(levels)),
    ]
}

/// A `NestingProbe` carrying `p` as written.
pub fn probe(id: &str, p: &Value) -> Value {
    json!({"id": id, "type": "NestingProbe", "p": p, "@context": [CORE_CONTEXT]})
}

/// The entity `id` in the normalized representation, or with `query`.
pub async fn read(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}: {:?}", response.body);
    response.json_value().expect("an entity body")
}

/// Assert that the entity `id` is not stored.
pub async fn assert_absent(client: &TestClient, id: &str) {
    let missing = client
        .get_entity(id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(
        missing.status,
        StatusCode::NOT_FOUND,
        "{id}: a refused write stores nothing: {:?}",
        missing.body
    );
}

/// Assert `response` is the refusal of `p` nesting deeper than [`LIMIT`].
pub fn assert_refused(response: &TestResponse, case: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{case}: {:?}", response.body);
    let problem = response.json_value().expect("a problem detail");
    assert_refusal_detail(&problem, case);
}

/// Assert `problem` is the problem detail of `p` nesting deeper than
/// [`LIMIT`].
pub fn assert_refusal_detail(problem: &Value, case: &str) {
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{case}: {problem}");
    let detail = problem["detail"].as_str().expect("a detail");
    assert!(
        detail.contains(&format!("'{NESTED}'")) && detail.contains(&format!("{LIMIT} levels")),
        "{case}: {detail}"
    );
}
