//! `Prefer: ngsi-ld=<version>` against a response that is not in the normalized
//! representation.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.3.6.8 amends a payload down to the version a
//! client named, through Table 4.3.6.8-1 (the Entity members and the attribute
//! families), Table 4.3.6.8-2 (the Property members) and Table 4.3.6.8-3 (the
//! Relationship members). The tables are applied to the payload as it already
//! stands, so which representation it was rendered in decides where an
//! attribute's family and value are read from: clause 4.5.1 reifies an
//! attribute into an object naming its `type`, clause 4.5.2.3 drops that `type`
//! and leaves the family to be inferred from the value member, and clause 4.5.4
//! reduces the attribute to its bare value or to one of three one-member
//! wrappers.
//!
//! Before this suite existed the response pipeline never told the adaptation
//! which representation it was working on, so every `Prefer: ngsi-ld=` response
//! was amended under the normalized reading. In a clause 4.5.4 payload the value
//! member that reading looks for is the wrapper key, so `languageMap`, `json`
//! and `vocab` were each renamed to `value` and a `keyValues` response came back
//! carrying `{"value": ...}` objects — a shape clause 4.5.4 defines for nothing.
//! In a concise payload the clause 4.5.2.3 collapse never ran, so an attribute
//! stripped down to its value alone stayed an object.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// An `expiresAt` far enough ahead that the expiry sweep never reaches it.
const FAR_FUTURE: &str = "2099-01-01T00:00:00Z";

/// The `datasetId` telling the two instances of the multi-instance attribute
/// apart (clause 4.5.5).
const DATASET: &str = "urn:ngsi-ld:Dataset:secondary";

/// The Relationship target the single-object families point at.
const BUILDING: &str = "urn:ngsi-ld:Building:target";

/// The two Relationship targets the `ListRelationship` points at.
const ROOMS: [&str; 2] = ["urn:ngsi-ld:Room:first", "urn:ngsi-ld:Room:second"];

/// An entity carrying one attribute of each of the eight families Table
/// 4.3.6.8-1 names, plus the Entity members and the multi-instance case its
/// notes name.
fn every_family(id: &str) -> Value {
    json!({
        "id": id,
        // Table 4.3.6.8-1 note 1: several Entity types exist from 1.3 onwards.
        "type": ["Vehicle", "Car"],
        // Table 4.3.6.8-1: `scope` was introduced at 1.4, entity `expiresAt` at 1.9.
        "scope": ["/conformance"],
        "expiresAt": FAR_FUTURE,
        // Table 4.3.6.8-2: `observedAt` and `unitCode` at 1.3, `valueType` at 1.9.
        "speed": {"type": "Property", "value": 21, "observedAt": "2025-01-01T00:00:00Z", "unitCode": "CEL", "valueType": "Number"},
        "spot": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
        // Table 4.3.6.8-3: `observedAt` at 1.3 and `objectType` at 1.8.
        "refBuilding": {"type": "Relationship", "object": BUILDING, "objectType": "Building", "observedAt": "2025-01-01T00:00:00Z"},
        // Table 4.3.6.8-1: the LanguageProperty was introduced at 1.4.
        "label": {"type": "LanguageProperty", "languageMap": {"en": "Store", "de": "Laden"}},
        // Table 4.3.6.8-1: VocabProperty, JsonProperty, ListProperty and
        // ListRelationship were all introduced at 1.8.
        "category": {"type": "VocabProperty", "vocab": "Building"},
        "config": {"type": "JsonProperty", "json": {"a": 1}},
        "tags": {"type": "ListProperty", "valueList": ["a", "b"]},
        "refRooms": {"type": "ListRelationship", "objectList": ROOMS},
        // Table 4.3.6.8-1 notes 2 and 3: several instances of one attribute,
        // told apart by the `datasetId` introduced at 1.3.
        "pressure": [
            {"type": "Property", "value": 25, "datasetId": DATASET},
            {"type": "Property", "value": 30}
        ],
        "@context": [CORE_CONTEXT]
    })
}

/// Create the entity every test reads back.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Read one entity under `selection` (the `format`/`options` query fragment),
/// optionally naming a version in `Prefer`.
async fn retrieve(client: &TestClient, id: &str, selection: &str, prefer: Option<&str>) -> (StatusCode, Value) {
    let path = format!("/ngsi-ld/v1/entities/{}?{selection}", urlencoding::encode(id));
    send(client, &path, prefer).await
}

/// Query for one entity under `selection`, optionally naming a version in
/// `Prefer`, and answer with the single entity the collection carries.
async fn query_one(client: &TestClient, id: &str, selection: &str, prefer: Option<&str>) -> (StatusCode, Value) {
    let path = format!("/ngsi-ld/v1/entities?id={}&{selection}", urlencoding::encode(id));
    let (status, body) = send(client, &path, prefer).await;
    let entities = body.as_array().expect("a query answers with an array");
    assert_eq!(entities.len(), 1, "exactly one entity must match: {body}");
    (status, entities[0].clone()) // clone: the assertions outlive the collection they were read from
}

/// Send one `application/json` read, optionally carrying a `Prefer` header.
async fn send(client: &TestClient, path: &str, prefer: Option<&str>) -> (StatusCode, Value) {
    let mut request = client.raw(Method::GET, path).header("Accept", "application/json");
    if let Some(prefer) = prefer {
        request = request.header("Prefer", prefer);
    }
    let response = request.send().await.expect("read request");
    let body = response.json_value().expect("a read answers with JSON");
    (response.status, body)
}

/// Every value an entity's attributes carry, so a shape can be looked for
/// across the whole payload rather than one member at a time.
fn attribute_values(entity: &Value) -> Vec<&Value> {
    let members = entity.as_object().expect("an entity is a JSON object");
    members
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "id" | "type" | "scope" | "expiresAt" | "@context"))
        .map(|(_, value)| value)
        .collect()
}

/// Whether a value is the one-member `{"value": ...}` object the normalized
/// reading of a clause 4.5.4 payload used to produce.
fn is_value_wrapper(value: &Value) -> bool {
    value.as_object().is_some_and(|members| members.len() == 1 && members.contains_key("value"))
}

/// Clause 4.5.4 and Tables 4.3.6.8-1 to -3 for a 1.0 target, family by family.
///
/// Clause 4.5.4 gives a Property "the Property Value", a `GeoProperty` the same,
/// a Relationship "the Relationship's Object (represented as a URI or array of
/// URIs)", and a `LanguageProperty`, `JsonProperty` and `VocabProperty` a JSON object
/// carrying the single member `languageMap`, `json` or `vocab`. Table 4.3.6.8-1
/// reformats the last three as Properties for a 1.0 target, and a Property in
/// this representation is its bare value, so each wrapper is removed and nothing
/// is renamed. The two list families reformat to a Property and a Relationship
/// whose clause 4.5.4 renderings are the arrays already on the wire.
#[tokio::test]
async fn regression_prefer_version_simplified_answers_the_clause_4_5_4_shape_for_every_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:prefer-simplified-families");
    create_entity(client, &every_family(&id)).await;

    let (unadapted_status, unadapted) = retrieve(client, &id, "options=keyValues", None).await;
    let (status, body) = retrieve(client, &id, "options=keyValues", Some("ngsi-ld=1.0")).await;

    // The control: without a preference the wrappers clause 4.5.4 mandates are
    // exactly what is on the wire, so each assertion below names a difference
    // the amendment made rather than a shape the rendering never had.
    assert_eq!(unadapted_status, StatusCode::OK, "{unadapted}");
    assert_eq!(unadapted["label"], json!({"languageMap": {"en": "Store", "de": "Laden"}}), "{unadapted}");
    assert_eq!(unadapted["category"], json!({"vocab": "Building"}), "{unadapted}");
    assert_eq!(unadapted["config"], json!({"json": {"a": 1}}), "{unadapted}");
    assert_eq!(unadapted["pressure"], json!({"dataset": {"@none": 30, DATASET: 25}}), "{unadapted}");

    // Clause 4.3.6.8: an amended payload is no longer authoritative.
    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");

    // Property and GeoProperty (1.0): "the Property Value", untouched.
    assert_eq!(body["speed"], json!(21), "{body}");
    assert_eq!(body["spot"], json!({"type": "Point", "coordinates": [1.0, 2.0]}), "{body}");
    // Relationship (1.0): the Object as a URI, untouched.
    assert_eq!(body["refBuilding"], json!(BUILDING), "{body}");
    // LanguageProperty (1.4), VocabProperty (1.8) and JsonProperty (1.8):
    // reformatted as Properties, so each wrapper unwraps to the bare value.
    assert_eq!(body["label"], json!({"en": "Store", "de": "Laden"}), "{body}");
    assert_eq!(body["category"], json!("Building"), "{body}");
    assert_eq!(body["config"], json!({"a": 1}), "{body}");
    // ListProperty and ListRelationship (1.8): a Property whose value is that
    // array, and a Relationship whose Object is that array of URIs, render as
    // the arrays already on the wire.
    assert_eq!(body["tags"], json!(["a", "b"]), "{body}");
    assert_eq!(body["refRooms"], json!([ROOMS[0], ROOMS[1]]), "{body}");
    // Table 4.3.6.8-1 note 2: one instance, "with preference to the default
    // instance", which clause 4.5.4 keys as `@none`.
    assert_eq!(body["pressure"], json!(30), "{body}");

    // Table 4.3.6.8-1: `scope` arrived at 1.4 and entity `expiresAt` at 1.9;
    // note 1 leaves a 1.0 target the first Entity type alone.
    assert!(body.get("scope").is_none(), "{body}");
    assert!(body.get("expiresAt").is_none(), "{body}");
    assert_eq!(body["type"], json!("Vehicle"), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// `{"value": X}` is a shape clause 4.5.4 defines for nothing: its Property
/// bullet is "a member whose key is the Property name (a term) and whose value
/// is the Property Value", and the three wrappers it does define are keyed
/// `languageMap`, `json` and `vocab`. No target may produce one.
#[tokio::test]
async fn regression_prefer_version_simplified_never_answers_a_value_wrapper() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:prefer-simplified-no-wrapper");
    create_entity(client, &every_family(&id)).await;

    for selection in ["options=keyValues", "format=simplified"] {
        for version in ["1.0", "1.3", "1.4", "1.8"] {
            let (_, body) = retrieve(client, &id, selection, Some(&format!("ngsi-ld={version}"))).await;
            let wrapped = attribute_values(&body).into_iter().any(is_value_wrapper);
            assert!(!wrapped, "{selection} at {version} answered a value wrapper: {body}");
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.3.6.8-1's "Version Introduced" column: a family is reformatted below
/// the version that introduced it and left alone at or above it. The
/// `LanguageProperty` arrived at 1.4 and the `VocabProperty` and `JsonProperty`
/// at 1.8, so a 1.4 target unwraps two of the three wrappers and keeps one.
#[tokio::test]
async fn regression_prefer_version_simplified_unwraps_only_below_the_version_that_introduced_the_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:prefer-simplified-thresholds");
    create_entity(client, &every_family(&id)).await;

    let (_, at_1_4) = retrieve(client, &id, "options=keyValues", Some("ngsi-ld=1.4")).await;
    let (_, at_1_8) = retrieve(client, &id, "options=keyValues", Some("ngsi-ld=1.8")).await;

    // 1.4 knew the LanguageProperty, the `scope` member and the `datasetId`
    // that separates instances, and knew neither the VocabProperty nor the
    // JsonProperty.
    assert_eq!(at_1_4["label"], json!({"languageMap": {"en": "Store", "de": "Laden"}}), "{at_1_4}");
    assert_eq!(at_1_4["category"], json!("Building"), "{at_1_4}");
    assert_eq!(at_1_4["config"], json!({"a": 1}), "{at_1_4}");
    assert_eq!(at_1_4["scope"], json!("/conformance"), "{at_1_4}");
    assert_eq!(at_1_4["pressure"], json!({"dataset": {"@none": 30, DATASET: 25}}), "{at_1_4}");

    // 1.8 knew every family; only the 1.9 Entity `expiresAt` is removed.
    assert_eq!(at_1_8["label"], json!({"languageMap": {"en": "Store", "de": "Laden"}}), "{at_1_8}");
    assert_eq!(at_1_8["category"], json!({"vocab": "Building"}), "{at_1_8}");
    assert_eq!(at_1_8["config"], json!({"json": {"a": 1}}), "{at_1_8}");
    assert!(at_1_8.get("expiresAt").is_none(), "{at_1_8}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.4's multi-attribute case carries the instances in "a JSON Object
/// with a key called `dataset`", keyed by `datasetId`, "where the default
/// datasetId (where present) is represented by the JSON-LD keyword `@none`".
/// Table 4.3.6.8-1 note 2 reduces that to one instance for a target below 1.3
/// and leaves it alone at 1.3, adapting each instance in its own right.
#[tokio::test]
async fn regression_prefer_version_simplified_reduces_a_dataset_map_to_its_default_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:prefer-simplified-dataset");
    let mut entity = every_family(&id);
    // A wrapped family in the map as well, so the 1.3 branch is seen adapting
    // the instances it keeps.
    entity["label"] = json!([
        {"type": "LanguageProperty", "languageMap": {"en": "Store"}, "datasetId": DATASET},
        {"type": "LanguageProperty", "languageMap": {"en": "Depot"}}
    ]);
    create_entity(client, &entity).await;

    let (_, at_1_0) = retrieve(client, &id, "options=keyValues", Some("ngsi-ld=1.0")).await;
    let (_, at_1_3) = retrieve(client, &id, "options=keyValues", Some("ngsi-ld=1.3")).await;

    assert_eq!(at_1_0["pressure"], json!(30), "{at_1_0}");
    assert_eq!(at_1_0["label"], json!({"en": "Depot"}), "{at_1_0}");

    assert_eq!(at_1_3["pressure"], json!({"dataset": {"@none": 30, DATASET: 25}}), "{at_1_3}");
    assert_eq!(
        at_1_3["label"],
        json!({"dataset": {"@none": {"en": "Depot"}, DATASET: {"en": "Store"}}}),
        "{at_1_3}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.3: "An NGSI-LD Property without sub-attributes shall be
/// represented in a concise but lossless representation by a member whose key is
/// the Property name and whose value is the Property Value." Removing the
/// members Table 4.3.6.8-2 names, or reformatting a `VocabProperty` into a
/// Property, leaves exactly that shape, so the collapse applies again after the
/// amendment. An array value does not collapse, because clause 4.5.5 would read
/// it back as several instances of the attribute.
#[tokio::test]
async fn regression_prefer_version_concise_collapses_a_reduced_property_to_its_bare_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:prefer-concise");
    create_entity(client, &every_family(&id)).await;

    let (unadapted_status, unadapted) = retrieve(client, &id, "options=concise", None).await;
    let (status, body) = retrieve(client, &id, "options=concise", Some("ngsi-ld=1.0")).await;

    assert_eq!(unadapted_status, StatusCode::OK, "{unadapted}");
    assert_eq!(unadapted["category"], json!({"vocab": "Building"}), "{unadapted}");
    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");

    // Table 4.3.6.8-2 takes `observedAt`, `unitCode` and `valueType` off the
    // Property, and what is left is the clause 4.5.2.3 bare value.
    assert_eq!(body["speed"], json!(21), "{body}");
    assert_eq!(body["category"], json!("Building"), "{body}");
    assert_eq!(body["config"], json!({"a": 1}), "{body}");
    assert_eq!(body["label"], json!({"en": "Store", "de": "Laden"}), "{body}");
    // Clause 4.5.5 reads a bare array as several instances, so the object form
    // whose `value` member clause 4.5.2.3 makes mandatory is kept.
    assert_eq!(body["tags"], json!({"value": ["a", "b"]}), "{body}");
    // Clause 4.5.3.3 keeps a concise Relationship's `object` member, and Table
    // 4.3.6.8-1 reformats the `ListRelationship` into one.
    assert_eq!(body["refBuilding"], json!({"object": BUILDING}), "{body}");
    assert_eq!(body["refRooms"], json!({"object": ROOMS[0]}), "{body}");
    assert_eq!(body["pressure"], json!(30), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// The normalized reading is unchanged by the representation being stated: a
/// clause 4.5.1 payload declares every attribute's `type`, and Table 4.3.6.8-1
/// reformats each family as the generalization it specializes.
#[tokio::test]
async fn regression_prefer_version_normalized_still_reformats_every_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:prefer-normalized");
    create_entity(client, &every_family(&id)).await;

    let (status, body) = retrieve(client, &id, "options=normalized", Some("ngsi-ld=1.0")).await;

    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");
    assert_eq!(body["speed"], json!({"type": "Property", "value": 21}), "{body}");
    assert_eq!(body["label"], json!({"type": "Property", "value": {"en": "Store", "de": "Laden"}}), "{body}");
    assert_eq!(body["category"], json!({"type": "Property", "value": "Building"}), "{body}");
    assert_eq!(body["config"], json!({"type": "Property", "value": {"a": 1}}), "{body}");
    assert_eq!(body["tags"], json!({"type": "Property", "value": ["a", "b"]}), "{body}");
    assert_eq!(body["refBuilding"], json!({"type": "Relationship", "object": BUILDING}), "{body}");
    assert_eq!(body["refRooms"], json!({"type": "Relationship", "object": ROOMS[0]}), "{body}");
    assert_eq!(body["pressure"], json!({"type": "Property", "value": 30}), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Query Entities renders the same representations Retrieve Entity does and is
/// a separate rendering call site, so the representation reaches the amendment
/// there too.
#[tokio::test]
async fn regression_prefer_version_simplified_answers_the_clause_4_5_4_shape_on_a_query_too() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:prefer-query");
    create_entity(client, &every_family(&id)).await;

    let (status, entity) = query_one(client, &id, "options=keyValues", Some("ngsi-ld=1.0")).await;

    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{entity}");
    assert_eq!(entity["label"], json!({"en": "Store", "de": "Laden"}), "{entity}");
    assert_eq!(entity["category"], json!("Building"), "{entity}");
    assert_eq!(entity["config"], json!({"a": 1}), "{entity}");
    assert_eq!(entity["pressure"], json!(30), "{entity}");
    assert!(entity.get("scope").is_none(), "{entity}");

    scope.finish().await.expect("cleanup failed");
}
