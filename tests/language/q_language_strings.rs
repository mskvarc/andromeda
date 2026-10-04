//! A Query Entities `q` reads a `LanguageProperty` through its `languageMap`
//! under every operator (clause 4.9).
//!
//! With a target language, "the target value is defined as the Value
//! associated to the matching key-value pair held within the languageMap …
//! where the key matches the target language"; with none it is "a value from
//! any of the key-value pairs". Either way the target value is a String, so the
//! ordering, pattern and range operators apply to it as they do to any String,
//! and a target element below a sub-attribute is read the same way.

use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// Create an entity of `entity_type` whose `name` is the language map `names`.
async fn create_named(client: &TestClient, id: &str, entity_type: &str, names: &Value) {
    let entity = json!({
        "id": id,
        "type": entity_type,
        "name": {"type": "LanguageProperty", "languageMap": names},
        "@context": [CORE_CONTEXT]
    });
    let response = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

/// The identifiers, in order, a query over `ids` with `q` returns.
async fn matching(client: &TestClient, entity_type: &str, ids: &[&str], q: &str) -> Vec<String> {
    let path = format!(
        "/ngsi-ld/v1/entities?type={entity_type}&id={}&q={}",
        urlencoding::encode(&ids.join(",")),
        urlencoding::encode(q)
    );
    let response = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(CORE_CONTEXT).expect("core context Link must be valid"))
        .send()
        .await
        .expect("query failed");
    assert_eq!(response.status, StatusCode::OK, "{q}");
    let body = response.json_value().expect("response body must be valid JSON");
    let mut found: Vec<String> = body
        .as_array()
        .expect("an entity array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    found.sort();
    found
}

/// The identifiers given, sorted the way [`matching`] sorts its answer.
fn sorted(ids: &[&str]) -> Vec<String> {
    let mut owned: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    owned.sort();
    owned
}

/// Clause 4.9 `patternOp` with a target language reads only that language's
/// string, and `notPatternOp` still requires the language to be present.
#[tokio::test]
async fn language_query_target_language_pattern_reads_only_that_language() {
    let scope = TestEnvironment::from_suite("language").await.expect("isolated test environment");
    let client = scope.client();
    let door = scope.id("R65LangA:door");
    let gate = scope.id("R65LangA:gate");
    let porte = scope.id("R65LangA:porte");
    create_named(client, &door, "R65LangA", &json!({"en": "Door", "de": "Tuer"})).await;
    create_named(client, &gate, "R65LangA", &json!({"en": "Gate", "de": "Dach"})).await;
    create_named(client, &porte, "R65LangA", &json!({"fr": "Porte"})).await;
    let ids = [door.as_str(), gate.as_str(), porte.as_str()];

    assert_eq!(matching(client, "R65LangA", &ids, "name[en]~=\"^D\"").await, sorted(&[&door]));
    assert_eq!(matching(client, "R65LangA", &ids, "name[de]~=\"^D\"").await, sorted(&[&gate]));
    assert_eq!(matching(client, "R65LangA", &ids, "name[en]!~=\"^D\"").await, sorted(&[&gate]));
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9's ordering operators and `Range` read the language's string as
/// any String target; the excluded range still requires the language.
#[tokio::test]
async fn language_query_target_language_ordering_and_range() {
    let scope = TestEnvironment::from_suite("language").await.expect("isolated test environment");
    let client = scope.client();
    let door = scope.id("R65LangB:door");
    let gate = scope.id("R65LangB:gate");
    let porte = scope.id("R65LangB:porte");
    create_named(client, &door, "R65LangB", &json!({"en": "Door", "de": "Zimmer"})).await;
    create_named(client, &gate, "R65LangB", &json!({"en": "Gate", "de": "Ausgang"})).await;
    create_named(client, &porte, "R65LangB", &json!({"fr": "Porte"})).await;
    let ids = [door.as_str(), gate.as_str(), porte.as_str()];

    assert_eq!(matching(client, "R65LangB", &ids, "name[en]>\"E\"").await, sorted(&[&gate]));
    assert_eq!(matching(client, "R65LangB", &ids, "name[en]<=\"Door\"").await, sorted(&[&door]));
    assert_eq!(matching(client, "R65LangB", &ids, "name[en]==\"C\"..\"E\"").await, sorted(&[&door]));
    assert_eq!(matching(client, "R65LangB", &ids, "name[en]!=\"C\"..\"E\"").await, sorted(&[&gate]));
    scope.finish().await.expect("cleanup failed");
}

/// With no target language every string is a candidate: `~=` holds when one
/// matches, `!~=` when none does, and a `ListProperty`'s array is never a
/// String target.
#[tokio::test]
async fn language_query_unqualified_pattern_reads_every_language() {
    let scope = TestEnvironment::from_suite("language").await.expect("isolated test environment");
    let client = scope.client();
    let door = scope.id("R65LangC:door");
    let gate = scope.id("R65LangC:gate");
    let list = scope.id("R65LangC:list");
    create_named(client, &door, "R65LangC", &json!({"en": "Door", "de": "Tuer"})).await;
    create_named(client, &gate, "R65LangC", &json!({"en": "Gate", "de": "Dach"})).await;
    let listed = json!({
        "id": list,
        "type": "R65LangC",
        "name": {"type": "ListProperty", "valueList": ["Tuer"]},
        "@context": [CORE_CONTEXT]
    });
    let response = client.post_entity(&listed).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
    let ids = [door.as_str(), gate.as_str(), list.as_str()];

    assert_eq!(matching(client, "R65LangC", &ids, "name~=\"^Tu\"").await, sorted(&[&door]));
    assert_eq!(matching(client, "R65LangC", &ids, "name!~=\"^Tu\"").await, sorted(&[&gate]));
    assert_eq!(matching(client, "R65LangC", &ids, "name[*]>\"S\"").await, sorted(&[&door]));
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9's target element is the last name of the dotted path, so a
/// `LanguageProperty` sub-attribute is read through its `languageMap` like a
/// top-level one, with or without a target language.
#[tokio::test]
async fn language_query_sub_attribute_language_property() {
    let scope = TestEnvironment::from_suite("language").await.expect("isolated test environment");
    let client = scope.client();
    let front = scope.id("R65LangD:front");
    let back = scope.id("R65LangD:back");
    for (id, labels) in [(&front, json!({"en": "Front", "de": "Vorne"})), (&back, json!({"en": "Back", "de": "Hinten"}))] {
        let entity = json!({
            "id": id,
            "type": "R65LangD",
            "door": {
                "type": "Property",
                "value": 1,
                "label": {"type": "LanguageProperty", "languageMap": labels}
            },
            "@context": [CORE_CONTEXT]
        });
        let response = client.post_entity(&entity).send().await.expect("create entity failed");
        assert_eq!(response.status, StatusCode::CREATED);
    }
    let ids = [front.as_str(), back.as_str()];

    assert_eq!(matching(client, "R65LangD", &ids, "door.label[en]==\"Front\"").await, sorted(&[&front]));
    assert_eq!(matching(client, "R65LangD", &ids, "door.label[de]==\"Front\"").await, Vec::<String>::new());
    assert_eq!(matching(client, "R65LangD", &ids, "door.label[en]!=\"Front\"").await, sorted(&[&back]));
    assert_eq!(matching(client, "R65LangD", &ids, "door.label[en]~=\"^F\"").await, sorted(&[&front]));
    assert_eq!(matching(client, "R65LangD", &ids, "door.label==\"Hinten\"").await, sorted(&[&back]));
    assert_eq!(matching(client, "R65LangD", &ids, "door.label[en]").await, sorted(&[&front, &back]));
    assert_eq!(matching(client, "R65LangD", &ids, "door.label[fr]").await, Vec::<String>::new());
    scope.finish().await.expect("cleanup failed");
}
