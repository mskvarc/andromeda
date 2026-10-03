//! A JSON literal in a Property value holding the NGSI-LD Null under a key
//! (R03).
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4: "Any attempt to use "urn:ngsi-ld:null"
//! as the value of a key value pair within a JSON object, which is the
//! right-hand side of the value of a Property, with the exception of NGSI-LD
//! Fragments used in merge operations (see clause 5.5.12), shall result in an
//! error of type `BadRequestData`."
//!
//! A JSON literal reaches a Property value as a member a context coerces to
//! `@json`, or as a value written `{"@type": "@json", "@value": ..}`. Either
//! way it is a JSON object on the right-hand side of the value. Before, Create
//! and Replace Entity stored the Null under its key as data, and every other
//! write outside a merge silently dropped the key. A merge deletes the key
//! (clause 5.5.12, IETF RFC 7396), which it already did.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

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
/// `@json`; the core context a read compacts with does not define it.
const SETTINGS: &str = "https://example.org/settings";

/// The two spellings of a JSON literal in the value of the Property `config`.
#[derive(Clone, Copy, Debug)]
enum Literal {
    /// The value object's `settings` member, which the context coerces to
    /// `@json`.
    Keyed,
    /// The value itself, written as a `@json` value object.
    Typed,
}

impl Literal {
    /// Both spellings.
    const ALL: [Literal; 2] = [Literal::Keyed, Literal::Typed];

    /// The Property `config` whose JSON literal is `json`.
    fn property(self, json: &Value) -> Value {
        match self {
            Literal::Keyed => json!({"type": "Property", "value": {"settings": json}}),
            Literal::Typed => json!({"type": "Property", "value": {"@type": "@json", "@value": json}}),
        }
    }

    /// The JSON literal `config` holds in `entity` as a read serves it.
    fn held(self, entity: &Value) -> &Value {
        match self {
            Literal::Keyed => &entity["config"]["value"][SETTINGS]["@value"],
            Literal::Typed => &entity["config"]["value"]["@value"],
        }
    }
}

/// The literal every held entity starts with.
fn held_literal() -> Value {
    json!({"a": 1, "b": 2})
}

/// The literal holding the Null under the key `b`.
fn null_keyed_literal() -> Value {
    json!({"a": 1, "b": NULL})
}

/// The request context: `settings` coerced to `@json`, then the core context.
fn context() -> Value {
    json!([{"settings": {"@id": SETTINGS, "@type": "@json"}}, CORE_CONTEXT])
}

/// `body` with the request context as its `@context`.
fn with_context(mut body: Value) -> Value {
    body["@context"] = context();
    body
}

/// The entity `id` carrying `config` as `literal` spells `json`.
fn entity(id: &str, literal: Literal, json: &Value) -> Value {
    with_context(json!({"id": id, "type": "LiteralProbe", "config": literal.property(json)}))
}

/// Send `body` as `application/ld+json` to the broker path `path`.
async fn send(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, &format!("/ngsi-ld/v1/{path}"))
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("write request")
}

/// Create the entity `id` holding [`held_literal`] in `literal`'s spelling.
async fn create_held(client: &TestClient, id: &str, literal: Literal) {
    let created = send(client, Method::POST, "entities", &entity(id, literal, &held_literal())).await;
    assert_eq!(created.status, StatusCode::CREATED, "{id}: {:?}", created.body);
}

/// The entity `id` as a read serves it, or `None` when it is not stored.
async fn stored(client: &TestClient, id: &str) -> Option<Value> {
    let response = client
        .get_entity(id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert!(matches!(response.status, StatusCode::OK | StatusCode::NOT_FOUND), "{id}: {:?}", response.body);
    (response.status == StatusCode::OK).then(|| response.json_value().expect("an entity body"))
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

/// Assert the entity `id` still holds [`held_literal`].
async fn assert_held(client: &TestClient, id: &str, literal: Literal, case: &str) {
    let served = stored(client, id).await.expect("the entity is stored");
    assert_eq!(literal.held(&served), &held_literal(), "{case}: {served}");
}

/// Clause 5.6.1: a creation is no fragment, so it is refused and stores
/// nothing, where it used to store the Null under the key as data.
#[tokio::test]
async fn regression_r03_a_create_refuses_a_json_literal_holding_the_null_under_a_key() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for literal in Literal::ALL {
        let case = format!("create {literal:?}");
        let id = scope.id(&format!("LiteralProbe:create-{literal:?}"));
        let created = send(client, Method::POST, "entities", &entity(&id, literal, &null_keyed_literal())).await;
        assert_refused(&created, &case);
        assert_eq!(stored(client, &id).await, None, "{case}: a refused create stores nothing");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.2, 5.6.3, 5.6.4, 5.6.18 and 5.6.19 are no merge, so each
/// refuses the literal and leaves the held one as it was, where the update
/// paths used to drop the key and Replace Entity to store the Null.
#[tokio::test]
async fn regression_r03_every_write_of_a_held_entity_refuses_a_json_literal_holding_the_null_under_a_key() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for literal in Literal::ALL {
        let attribute = with_context(literal.property(&null_keyed_literal()));
        let fragment = with_context(json!({"config": literal.property(&null_keyed_literal())}));
        let writes: [(&str, Method, &str, Value); 5] = [
            (
                "replace entity",
                Method::PUT,
                "",
                with_context(json!({"type": "LiteralProbe", "config": literal.property(&null_keyed_literal())})),
            ),
            ("update attributes", Method::PATCH, "/attrs", fragment.clone()), // clone: two writes send the same fragment
            ("append attributes", Method::POST, "/attrs", fragment),
            ("partial attribute update", Method::PATCH, "/attrs/config", attribute.clone()), // clone: two writes send the same attribute
            ("replace attribute", Method::PUT, "/attrs/config", attribute),
        ];
        for (write, method, suffix, body) in writes {
            let case = format!("{write} {literal:?}");
            let id = scope.id(&format!("LiteralProbe:{}-{literal:?}", write.replace(' ', "-")));
            create_held(client, &id, literal).await;
            let path = format!("entities/{}{suffix}", urlencoding::encode(&id));
            let response = send(client, method, &path, &body).await;
            assert_refused(&response, &case);
            assert_held(client, &id, literal, &case).await;
        }
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.7 to 5.6.9: each batch runs the single-entity behaviour per
/// entity, so only the entity carrying the literal is refused, as its own
/// `BatchEntityError`, and its neighbour is written.
#[tokio::test]
async fn regression_r03_every_batch_write_refuses_only_the_entity_holding_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (operation, held) in [("create", false), ("upsert", true), ("upsert?options=update", true), ("update", true)] {
        let case = format!("batch {operation}");
        let local = operation.replace(['?', '='], "-");
        let refused = scope.id(&format!("LiteralProbe:batch-{local}-refused"));
        let written = scope.id(&format!("LiteralProbe:batch-{local}-written"));
        if held {
            create_held(client, &refused, Literal::Keyed).await;
            create_held(client, &written, Literal::Keyed).await;
        }
        let body = json!([
            entity(&refused, Literal::Keyed, &null_keyed_literal()),
            entity(&written, Literal::Keyed, &json!({"a": 3})),
        ]);
        let response = send(client, Method::POST, &format!("entityOperations/{operation}"), &body).await;
        assert_eq!(response.status, StatusCode::MULTI_STATUS, "{case}: {:?}", response.body);
        let result = response.json_value().expect("a batch result");
        assert_eq!(result["success"], json!([written]), "{case}: {result}");
        let errors = result["errors"].as_array().expect("an errors array");
        assert_eq!(errors.len(), 1, "{case}: {result}");
        assert_eq!(errors[0]["entityId"], json!(refused), "{case}: {result}");
        assert_eq!(errors[0]["error"]["type"], json!(BAD_REQUEST_DATA), "{case}: {result}");
        if held {
            assert_held(client, &refused, Literal::Keyed, &case).await;
        } else {
            assert_eq!(stored(client, &refused).await, None, "{case}: a refused create stores nothing");
        }
        let neighbour = stored(client, &written).await.expect("the neighbour is written");
        assert_eq!(Literal::Keyed.held(&neighbour), &json!({"a": 3}), "{case}: {neighbour}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.5.12 excepts merge fragments: Merge Entity and Batch Entity Merge
/// delete the key the literal sets to the Null.
#[tokio::test]
async fn regression_r03_a_merge_deletes_the_key_a_json_literal_sets_to_the_null() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for literal in Literal::ALL {
        let single = scope.id(&format!("LiteralProbe:merge-{literal:?}"));
        create_held(client, &single, literal).await;
        let fragment = with_context(json!({"config": literal.property(&null_keyed_literal())}));
        let merged = send(client, Method::PATCH, &format!("entities/{}", urlencoding::encode(&single)), &fragment).await;
        assert_eq!(merged.status, StatusCode::NO_CONTENT, "merge {literal:?}: {:?}", merged.body);
        let served = stored(client, &single).await.expect("the merged entity");
        assert_eq!(literal.held(&served), &json!({"a": 1}), "merge {literal:?}: {served}");

        let batched = scope.id(&format!("LiteralProbe:batch-merge-{literal:?}"));
        create_held(client, &batched, literal).await;
        let body = json!([entity(&batched, literal, &null_keyed_literal())]);
        let merged = send(client, Method::POST, "entityOperations/merge", &body).await;
        assert_eq!(merged.status, StatusCode::NO_CONTENT, "batch merge {literal:?}: {:?}", merged.body);
        let served = stored(client, &batched).await.expect("the merged entity");
        assert_eq!(literal.held(&served), &json!({"a": 1}), "batch merge {literal:?}: {served}");
    }

    scope.finish().await.expect("environment cleanup");
}
