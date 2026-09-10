//! The Scope in the Temporal Evolution of an Entity (clauses 4.18 and 4.5.6).
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.18: "The Scope is represented as a special
//! scope Property that is non-reified in the normalized Entity representation
//! and reified in the temporal representation of an Entity. [...] For the
//! Temporal Evolution a given Scope is considered valid from the time it has
//! been set until the time it has been explicitly removed by an update or
//! delete operation (for an example see annex C, clause C.5.16)."
//!
//! Two defects sat behind that sentence. The reified Scope never reached a
//! temporal representation at all — scope observations were written and never
//! read back — and the temporal `scopeQ` tested the instant a Scope was set for
//! membership of the query window rather than testing the interval the Scope
//! was valid over, so annex C clause C.5.16's Vehicle B9211, scoped at 11:00
//! and queried between 12:00 and 13:00, matched nothing.
//!
//! Clause 4.5.6 settles what the reified form carries: the Scope "shall be
//! represented as a temporal representation of a Property (clause 4.5.7) that
//! can only have the non-reified Temporal Properties createdAt, modifiedAt,
//! deletedAt and observedAt as sub-Properties. [...] In case the Temporal
//! Evolution of the Scope is updated as the result of a change from the Core
//! API, the observedAt sub-Property should be set as a copy of the modifiedAt
//! sub-Property."
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Map, Value, json};

/// The core context the temporal payloads of this module are written against,
/// which is the one annex C clause C.5.16 answers under.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The Scope annex C clause C.5.16 queries for.
const MADRID_CENTRO: &str = "/Madrid/Centro";

/// The second Scope annex C clause C.5.16 gives Vehicle A8311.
const COMPANY_UNIT: &str = "/Company123/UnitA";

/// The NGSI-LD Null, which clause 4.18 spells a removed Scope with.
const NGSI_LD_NULL: &str = "urn:ngsi-ld:null";

/// The window annex C clause C.5.16 queries: 1 August 2018, noon to 1 PM.
const WINDOW: &str = "timerel=between&timeAt=2018-08-01T12:00:00Z&endTimeAt=2018-08-01T13:00:00Z";

/// One reified scope instance, in the temporal representation of a Property
/// clause 4.5.6 mandates.
fn scope_instance(value: Value, observed_at: &str) -> Value {
    // Built member by member rather than through `json!` interpolation, which
    // would borrow `value` and copy it back out again.
    let mut instance = Map::new();
    instance.insert("type".to_owned(), json!("Property"));
    instance.insert("value".to_owned(), value);
    instance.insert("observedAt".to_owned(), json!(observed_at));
    Value::Object(instance)
}

/// One `speed` instance at the given instant.
fn speed(value: i64, observed_at: &str) -> Value {
    json!({"type": "Property", "value": value, "observedAt": observed_at})
}

/// One Temporal Evolution of a `Vehicle`, as clause 5.7.6 accepts it.
fn vehicle(id: &str, scope: Value, speeds: Vec<Value>) -> Value {
    // As in `scope_instance`, the members are moved in rather than interpolated.
    let mut entity = Map::new();
    entity.insert("id".to_owned(), json!(id));
    entity.insert("type".to_owned(), json!("Vehicle"));
    entity.insert("scope".to_owned(), scope);
    entity.insert("speed".to_owned(), Value::Array(speeds));
    entity.insert("@context".to_owned(), json!(CORE_CONTEXT));
    Value::Object(entity)
}

/// Create one Temporal Evolution and assert clause 6.20.3.1 answered with 201.
async fn create_temporal(client: &TestClient, body: &Value) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// Query the Temporal Evolutions the given query string selects.
async fn query_temporal(client: &TestClient, query: &str) -> Vec<Value> {
    let response = client
        .query_temporal_entities(query, Some(CORE_CONTEXT))
        .await
        .expect("query temporal entities request");
    assert_eq!(response.status, StatusCode::OK, "query {query}: {:?}", response.body);
    let body = response.json_value().expect("the temporal query answers with JSON");
    body.as_array().cloned().unwrap_or_else(|| vec![body])
}

/// Retrieve one Temporal Evolution.
async fn retrieve_temporal(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_temporal_entity(id, query, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id} ({query}): {:?}", response.body);
    response.json_value().expect("the temporal retrieval answers with JSON")
}

/// The entity of a temporal result set carrying the given identifier.
fn entity_named<'a>(entities: &'a [Value], id: &str) -> Option<&'a Value> {
    entities.iter().find(|entity| entity["id"] == json!(id))
}

/// The `[value, instant]` pairs a clause 4.5.9 `temporalValues` member holds.
fn value_pairs(member: &Value) -> Vec<(Value, Value)> {
    member["values"]
        .as_array()
        .expect("a temporalValues member holds a values array")
        .iter()
        .map(|pair| {
            let pair = pair.as_array().expect("each element is a two-element array");
            (pair[0].clone(), pair[1].clone())
        })
        .collect()
}

/// The instances of one attribute of a normalized temporal representation.
fn instances(entity: &Value, member: &str) -> Vec<Value> {
    match &entity[member] {
        Value::Array(instances) => instances.clone(),
        Value::Null => Vec::new(),
        single @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => vec![single.clone()],
    }
}

// ---------------------------------------------------------------------------
// Annex C, clause C.5.16
// ---------------------------------------------------------------------------

/// Annex C, clause C.5.16, reproduced: "Give back the speed of all the Entities
/// of type "Vehicle" that have been within the Scope /Madrid/Centro between the
/// 1st of August 2018 at noon and the 1st of August 2018 at 01 PM. Note that
/// the value of the Scope has to match for the given timeframe, which means it
/// is possible that it has been set before, e.g. on 1st of August 2018 at
/// 11 AM."
///
/// The annex's own closing note is the second half of what is asserted here:
/// "Vehicle B9211 has already been within the Scope /Madrid/Centro before the
/// beginning of the request interval, whereas Vehicle A8311 only entered the
/// Scope within the request interval. Thus in the latter case only Property
/// values are included that have been observed after the Scope has become
/// valid."
#[tokio::test]
async fn regression_temporal_scope_annex_c_5_16_carries_the_scope_forward_and_clips_the_values() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let b9211 = scope.id("Vehicle:B9211");
    let a8311 = scope.id("Vehicle:A8311");

    // B9211 entered /Madrid/Centro an hour before the window opened.
    create_temporal(
        client,
        &vehicle(
            &b9211,
            scope_instance(json!(MADRID_CENTRO), "2018-08-01T11:00:00Z"),
            vec![
                speed(30, "2018-08-01T12:03:00Z"),
                speed(60, "2018-08-01T12:05:00Z"),
                speed(50, "2018-08-01T12:07:00Z"),
            ],
        ),
    )
    .await;

    // A8311 entered both Scopes inside the window, at 12:10, and had already
    // been observed at 12:05 — before the Scope became valid.
    create_temporal(
        client,
        &vehicle(
            &a8311,
            scope_instance(json!([MADRID_CENTRO, COMPANY_UNIT]), "2018-08-01T12:10:00Z"),
            vec![
                speed(11, "2018-08-01T12:05:00Z"),
                speed(40, "2018-08-01T12:12:00Z"),
                speed(60, "2018-08-01T12:14:00Z"),
                speed(50, "2018-08-01T12:16:00Z"),
            ],
        ),
    )
    .await;

    let entities = query_temporal(
        client,
        &format!("type=Vehicle&attrs=speed,scope&{WINDOW}&scopeQ=%2FMadrid%2FCentro&options=temporalValues"),
    )
    .await;

    let first = entity_named(&entities, &b9211).unwrap_or_else(|| panic!("B9211 is in the result: {entities:?}"));
    assert_eq!(
        value_pairs(&first["scope"]),
        vec![(json!(MADRID_CENTRO), json!("2018-08-01T11:00:00Z"))],
        "clause 4.18 carries a Scope set before the window into it: {first}"
    );
    assert_eq!(
        value_pairs(&first["speed"]),
        vec![
            (json!(30), json!("2018-08-01T12:03:00Z")),
            (json!(60), json!("2018-08-01T12:05:00Z")),
            (json!(50), json!("2018-08-01T12:07:00Z")),
        ],
        "B9211 was in the Scope for the whole window: {first}"
    );

    let second = entity_named(&entities, &a8311).unwrap_or_else(|| panic!("A8311 is in the result: {entities:?}"));
    assert_eq!(
        value_pairs(&second["scope"]),
        vec![(json!([MADRID_CENTRO, COMPANY_UNIT]), json!("2018-08-01T12:10:00Z"))],
        "clause 4.18 renders more than one Scope as a JSON array in one instance: {second}"
    );
    assert_eq!(
        value_pairs(&second["speed"]),
        vec![
            (json!(40), json!("2018-08-01T12:12:00Z")),
            (json!(60), json!("2018-08-01T12:14:00Z")),
            (json!(50), json!("2018-08-01T12:16:00Z")),
        ],
        "only the values observed after the Scope became valid are included: {second}"
    );

    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// The clause 4.18 validity interval a temporal scopeQ tests
// ---------------------------------------------------------------------------

/// Clause 4.18: a Scope "is considered valid from the time it has been set
/// until the time it has been explicitly removed". A Scope set before the
/// window and never removed is therefore valid throughout it, which is the core
/// of the defect: the probe tested the event instant for membership of the
/// window and answered with nothing.
#[tokio::test]
async fn regression_temporal_scope_set_before_the_window_and_never_removed_matches_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:scope-before-window");
    create_temporal(
        client,
        &vehicle(
            &id,
            scope_instance(json!(MADRID_CENTRO), "2018-08-01T09:00:00Z"),
            vec![speed(30, "2018-08-01T12:30:00Z")],
        ),
    )
    .await;

    let entities = query_temporal(client, &format!("type=Vehicle&attrs=speed&{WINDOW}&scopeQ=%2FMadrid%2FCentro")).await;
    assert!(
        entity_named(&entities, &id).is_some(),
        "a Scope set before the window is valid inside it: {entities:?}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A Scope that had already been removed when the window opened was not valid
/// during it, so clause 4.18 keeps the Entity out of the result even though the
/// Scope was once set.
#[tokio::test]
async fn regression_temporal_scope_removed_before_the_window_does_not_match_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:scope-removed-before-window");
    create_temporal(
        client,
        &vehicle(
            &id,
            json!([
                scope_instance(json!(MADRID_CENTRO), "2018-08-01T09:00:00Z"),
                scope_instance(json!(NGSI_LD_NULL), "2018-08-01T10:00:00Z"),
            ]),
            vec![speed(30, "2018-08-01T12:30:00Z")],
        ),
    )
    .await;

    let entities = query_temporal(client, &format!("type=Vehicle&attrs=speed&{WINDOW}&scopeQ=%2FMadrid%2FCentro")).await;
    assert!(
        entity_named(&entities, &id).is_none(),
        "a Scope removed before the window is not valid inside it: {entities:?}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A Scope both set and removed inside the window was valid for part of it, so
/// its interval overlaps the window and clause 4.18 keeps the Entity.
#[tokio::test]
async fn regression_temporal_scope_set_and_removed_inside_the_window_matches_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:scope-inside-window");
    create_temporal(
        client,
        &vehicle(
            &id,
            json!([
                scope_instance(json!(MADRID_CENTRO), "2018-08-01T12:10:00Z"),
                scope_instance(json!(NGSI_LD_NULL), "2018-08-01T12:40:00Z"),
            ]),
            vec![speed(30, "2018-08-01T12:20:00Z")],
        ),
    )
    .await;

    let entities = query_temporal(client, &format!("type=Vehicle&attrs=speed&{WINDOW}&scopeQ=%2FMadrid%2FCentro")).await;
    assert!(
        entity_named(&entities, &id).is_some(),
        "a Scope valid for part of the window matches it: {entities:?}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A Scope that only became valid after the window closed was not valid during
/// it, so the interval does not overlap and the Entity stays out.
#[tokio::test]
async fn regression_temporal_scope_set_after_the_window_does_not_match_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:scope-after-window");
    create_temporal(
        client,
        &vehicle(
            &id,
            scope_instance(json!(MADRID_CENTRO), "2018-08-01T14:00:00Z"),
            vec![speed(30, "2018-08-01T12:30:00Z")],
        ),
    )
    .await;

    let entities = query_temporal(client, &format!("type=Vehicle&attrs=speed&{WINDOW}&scopeQ=%2FMadrid%2FCentro")).await;
    assert!(
        entity_named(&entities, &id).is_none(),
        "a Scope set after the window closed was not valid inside it: {entities:?}"
    );

    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// The reified representation of clause 4.5.6
// ---------------------------------------------------------------------------

/// Clause 4.18 reifies the Scope in the temporal representation, and clause
/// 4.5.6 gives that reified form the four non-reified Temporal Properties
/// `createdAt`, `modifiedAt`, `deletedAt` and `observedAt`. The defect left the
/// member out of the representation altogether.
#[tokio::test]
async fn regression_temporal_scope_renders_reified_with_its_temporal_properties() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:reified-scope");
    create_temporal(
        client,
        &vehicle(
            &id,
            scope_instance(json!(MADRID_CENTRO), "2018-08-01T11:00:00Z"),
            vec![speed(30, "2018-08-01T12:03:00Z")],
        ),
    )
    .await;

    let entity = retrieve_temporal(client, &id, &format!("{WINDOW}&options=sysAttrs")).await;
    let scope_instances = instances(&entity, "scope");
    assert_eq!(scope_instances.len(), 1, "one instance of the scope Property (clause 4.18): {entity}");
    let instance = &scope_instances[0];
    assert_eq!(
        instance["type"],
        json!("Property"),
        "clause 4.5.6 makes it a temporal representation of a Property: {instance}"
    );
    assert_eq!(instance["value"], json!(MADRID_CENTRO), "{instance}");
    assert_eq!(
        instance["observedAt"],
        json!("2018-08-01T11:00:00Z"),
        "the instant the Scope was set: {instance}"
    );
    assert!(
        instance.get("createdAt").is_some(),
        "clause 4.5.6 gives the reified Scope createdAt: {instance}"
    );
    assert!(
        instance.get("modifiedAt").is_some(),
        "clause 4.5.6 gives the reified Scope modifiedAt: {instance}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.9 collapses each attribute of a temporal representation into
/// `[value, instant]` pairs, and the reified Scope is an attribute-shaped
/// member, so it collapses with them — which is the shape annex C clause
/// C.5.16 answers in.
#[tokio::test]
async fn regression_temporal_scope_renders_reified_in_temporal_values() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:reified-scope-values");
    create_temporal(
        client,
        &vehicle(
            &id,
            scope_instance(json!(MADRID_CENTRO), "2018-08-01T11:00:00Z"),
            vec![speed(30, "2018-08-01T12:03:00Z")],
        ),
    )
    .await;

    let entity = retrieve_temporal(client, &id, &format!("{WINDOW}&options=temporalValues")).await;
    assert_eq!(entity["scope"]["type"], json!("Property"), "{entity}");
    assert_eq!(
        value_pairs(&entity["scope"]),
        vec![(json!(MADRID_CENTRO), json!("2018-08-01T11:00:00Z"))],
        "{entity}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.18 reserves `urn:ngsi-ld:null` for a removed Scope, and clause
/// 4.5.7 records a deleted instance "with its value set to the URI
/// "urn:ngsi-ld:null" and the deletedAt Temporal Property set".
#[tokio::test]
async fn regression_temporal_scope_removal_renders_the_null_beside_its_deleted_at() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:removed-scope");
    create_temporal(
        client,
        &vehicle(
            &id,
            json!([
                scope_instance(json!(MADRID_CENTRO), "2018-08-01T12:10:00Z"),
                scope_instance(json!(NGSI_LD_NULL), "2018-08-01T12:40:00Z"),
            ]),
            vec![speed(30, "2018-08-01T12:20:00Z")],
        ),
    )
    .await;

    let entity = retrieve_temporal(client, &id, WINDOW).await;
    let scope_instances = instances(&entity, "scope");
    let removal = scope_instances
        .iter()
        .find(|instance| instance["value"] == json!(NGSI_LD_NULL))
        .unwrap_or_else(|| panic!("the removal is in the representation: {entity}"));
    assert!(
        removal.get("deletedAt").is_some(),
        "clause 4.5.7 sets deletedAt on a deleted instance: {removal}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.18: "The value of scope is represented as a JSON array in case
/// there is more than one Scope", and "There shall at most be one instance of
/// the scope property per Entity", so a write naming two Scopes is one instance
/// holding both — not two instances.
#[tokio::test]
async fn regression_temporal_scope_several_scopes_render_as_one_instance_holding_the_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:several-scopes");
    create_temporal(
        client,
        &vehicle(
            &id,
            scope_instance(json!([MADRID_CENTRO, COMPANY_UNIT]), "2018-08-01T12:10:00Z"),
            vec![speed(30, "2018-08-01T12:20:00Z")],
        ),
    )
    .await;

    let entity = retrieve_temporal(client, &id, WINDOW).await;
    let scope_instances = instances(&entity, "scope");
    assert_eq!(scope_instances.len(), 1, "clause 4.18 allows one instance per Entity: {entity}");
    assert_eq!(
        scope_instances[0]["value"],
        json!([MADRID_CENTRO, COMPANY_UNIT]),
        "both Scopes are the value of the one instance: {entity}"
    );

    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Table 6.18.3.2-1: the `attrs` synonym
// ---------------------------------------------------------------------------

/// Table 6.18.3.2-1 makes `attrs` on a temporal query "a synonym for a
/// combination of the pick and q parameters", and `pick` names Entity members —
/// "id", "type", "scope" or an Attribute name. Annex C clause C.5.16 queries
/// with `attrs=speed,scope`, which the broker refused outright.
#[tokio::test]
async fn regression_temporal_scope_attrs_selects_the_scope_beside_an_attribute() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:attrs-scope");
    create_temporal(
        client,
        &vehicle(
            &id,
            scope_instance(json!(MADRID_CENTRO), "2018-08-01T11:00:00Z"),
            vec![speed(30, "2018-08-01T12:03:00Z")],
        ),
    )
    .await;

    let entities = query_temporal(client, &format!("type=Vehicle&attrs=speed,scope&{WINDOW}")).await;
    let entity = entity_named(&entities, &id).unwrap_or_else(|| panic!("the entity is in the result: {entities:?}"));
    assert!(!instances(entity, "scope").is_empty(), "attrs=speed,scope keeps the Scope: {entity}");
    assert!(!instances(entity, "speed").is_empty(), "attrs=speed,scope keeps speed: {entity}");

    scope.finish().await.expect("cleanup failed");
}

/// An `attrs` that names an Attribute and not `scope` narrows the temporal
/// representation to that Attribute, so the Scope is left out.
#[tokio::test]
async fn regression_temporal_scope_attrs_without_the_scope_member_leaves_it_out() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:attrs-no-scope");
    create_temporal(
        client,
        &vehicle(
            &id,
            scope_instance(json!(MADRID_CENTRO), "2018-08-01T11:00:00Z"),
            vec![speed(30, "2018-08-01T12:03:00Z")],
        ),
    )
    .await;

    let entities = query_temporal(client, &format!("type=Vehicle&attrs=speed&{WINDOW}")).await;
    let entity = entity_named(&entities, &id).unwrap_or_else(|| panic!("the entity is in the result: {entities:?}"));
    assert!(instances(entity, "scope").is_empty(), "attrs=speed does not select the Scope: {entity}");

    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Clause 4.18: the operations that end a Scope's validity
// ---------------------------------------------------------------------------

/// Clause 5.6.18.4 replaces an Entity "with the new Entity content provided",
/// so a replacement that omits `scope` leaves the Entity with none — and clause
/// 4.18 ends the Scope's validity at that "update or delete operation". Without
/// a recorded removal the reconstructed interval stayed open for ever and a
/// later window still reported the Scope.
#[tokio::test]
async fn regression_temporal_scope_replace_omitting_the_member_closes_the_interval() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:replace-drops-scope");
    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Vehicle",
            "scope": MADRID_CENTRO,
            "speed": {"type": "Property", "value": 30},
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let replaced = client
        .replace_entity(
            &id,
            &json!({
                "id": id,
                "type": "Vehicle",
                "speed": {"type": "Property", "value": 40},
                "@context": CORE_CONTEXT
            }),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace entity request");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);

    let entity = retrieve_temporal(client, &id, "").await;
    let removal = instances(&entity, "scope")
        .into_iter()
        .find(|instance| instance["value"] == json!(NGSI_LD_NULL));
    let removal = removal.unwrap_or_else(|| panic!("the replacement records the removal clause 4.18 calls for: {entity}"));
    assert!(removal.get("deletedAt").is_some(), "the removal carries its instant: {removal}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.18 ends a Scope's validity at a "delete operation" as well as at an
/// update, and clause 5.6.6 leaves the Temporal Evolution standing when the
/// Entity is deleted, so the interval has to close rather than outlive the
/// Entity that carried it.
#[tokio::test]
async fn regression_temporal_scope_entity_deletion_closes_the_interval() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:deleted-with-scope");
    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Vehicle",
            "scope": MADRID_CENTRO,
            "speed": {"type": "Property", "value": 30},
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let deleted = client.delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let entity = retrieve_temporal(client, &id, "").await;
    let removal = instances(&entity, "scope")
        .into_iter()
        .find(|instance| instance["value"] == json!(NGSI_LD_NULL));
    assert!(removal.is_some(), "clause 4.18 ends the Scope at the delete operation: {entity}");

    scope.finish().await.expect("cleanup failed");
}

/// A Scope that a later write replaced is superseded at that write, so a window
/// opening after it reports the replacement alone — the interval of the first
/// Scope ended when the second began.
#[tokio::test]
async fn regression_temporal_scope_a_replacement_supersedes_the_scope_before_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:superseded-scope");
    create_temporal(
        client,
        &vehicle(
            &id,
            json!([
                scope_instance(json!(MADRID_CENTRO), "2018-08-01T09:00:00Z"),
                scope_instance(json!(COMPANY_UNIT), "2018-08-01T10:00:00Z"),
            ]),
            vec![speed(30, "2018-08-01T12:30:00Z")],
        ),
    )
    .await;

    let entity = retrieve_temporal(client, &id, WINDOW).await;
    let values: Vec<Value> = instances(&entity, "scope").into_iter().map(|instance| instance["value"].clone()).collect();
    assert_eq!(
        values,
        vec![json!(COMPANY_UNIT)],
        "only the Scope in force when the window opened is carried forward: {entity}"
    );

    let matched = query_temporal(client, &format!("type=Vehicle&attrs=speed&{WINDOW}&scopeQ=%2FMadrid%2FCentro")).await;
    assert!(
        entity_named(&matched, &id).is_none(),
        "the superseded Scope was not valid during the window: {matched:?}"
    );

    scope.finish().await.expect("cleanup failed");
}
