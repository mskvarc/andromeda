//! Where the broker admits the NGSI-LD Null, and where it must refuse it.
//!
//! Clause 5.2.1 gives `urn:ngsi-ld:null` meaning "in the context of a partial
//! update or merge operation (see clauses 5.5.8 and 5.5.12)" and adds: "In all
//! other cases, implementations shall raise an error of type `BadRequestData`
//! if an NGSI-LD Null value is encountered." Clause 5.5.4 states that as three
//! prohibitions whose exception lists differ — the nested one is excepted for
//! merge operations alone — and clauses 5.5.8 and 5.5.12 add one that has no
//! exception at all: "A `datasetId` cannot be deleted by setting it to the value
//! `urn:ngsi-ld:null`."
//!
//! The defects these cover: a `datasetId` set to the sentinel was stored and
//! served back (and a merge silently deleted the member, moving the instance
//! onto the default one); the batch operations ran no null validation at all,
//! so a payload `POST /entities` refused was created verbatim by
//! `POST /entityOperations/create`; and a null nested inside a Property value
//! object was accepted on every write path rather than on merge alone; and a
//! Null standing beside data in one member was stored as a value the broker
//! then failed to read back.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use urlencoding::encode;

/// The NGSI-LD Null of clause 4.5.0.
const NULL_URI: &str = "urn:ngsi-ld:null";

/// The problem type clause 5.5.4 gives an NGSI-LD Null an operation does not
/// admit.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The `datasetId` the multi-attribute fixtures are tagged with (clause 4.5.5).
const DATASET_A: &str = "urn:ngsi-ld:Dataset:A";

/// Assert a refusal is the `BadRequestData` of clauses 5.2.1 and 5.5.4.
fn assert_bad_request_data(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// Assert a batch refused every item it was given, each as its own
/// `BadRequestData` (clause 5.2.17), and created nothing.
fn assert_batch_refused_every_entity(response: &TestResponse, expected: usize, scenario: &str) {
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "{scenario}: {:?}", response.body);
    let result = response.json_value().expect("a partly failed batch carries a result body");
    assert_eq!(result["success"], json!([]), "{scenario}: nothing may be written: {result}");
    let errors = result["errors"].as_array().expect("a batch result carries an errors array");
    assert_eq!(errors.len(), expected, "{scenario}: one error per refused entity: {result}");
    for error in errors {
        assert_eq!(error["error"]["type"], json!(BAD_REQUEST_DATA), "{scenario}: {error}");
    }
}

/// A `Building` carrying exactly one attribute.
fn building_with(id: &str, attribute: &str, node: &Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        attribute: node,
        "@context": [CORE_CONTEXT]
    })
}

/// The `address` Property clause 5.5.12 EXAMPLE 2 is written over.
fn address_node() -> Value {
    json!({"type": "Property", "value": {"street": "Strasse des 17. Juni", "city": "Berlin", "country": "Germany"}})
}

/// A `temperature` Property tagged with [`DATASET_A`], so the entity holds a
/// named instance rather than the default one.
fn temperature_in_dataset_a() -> Value {
    json!({"type": "Property", "value": 21.5, "datasetId": DATASET_A})
}

/// One entity per prohibition of clause 5.5.4, so one batch exercises all
/// three: the first level member value, the right-hand side of a Property
/// `value`, and a key inside the JSON object that value holds.
fn entities_carrying_every_prohibited_form(scope: &TestEnvironment, local: &str) -> (Value, Vec<String>) {
    let ids = vec![
        scope.id(&format!("Building:{local}-first-level")),
        scope.id(&format!("Building:{local}-value")),
        scope.id(&format!("Building:{local}-nested")),
    ];
    let body = json!([
        building_with(&ids[0], "temperature", &json!(NULL_URI)),
        building_with(&ids[1], "temperature", &json!({"type": "Property", "value": NULL_URI})),
        building_with(&ids[2], "address", &json!({"type": "Property", "value": {"country": NULL_URI}})),
    ]);
    (body, ids)
}

/// Create one entity and assert the broker stored it.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Read one entity back in the normalized representation.
async fn stored_entity(client: &TestClient, id: &str) -> Value {
    let retrieved = client
        .get_entity(id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    retrieved.json_value().expect("a retrieved entity is JSON")
}

/// Assert none of `ids` names a stored entity: a refused write must leave
/// nothing behind.
async fn assert_none_stored(client: &TestClient, ids: &[String], scenario: &str) {
    for id in ids {
        let retrieved = client
            .get_entity(id, "", "application/ld+json", Some(CORE_CONTEXT))
            .await
            .expect("retrieve entity request");
        assert_eq!(retrieved.status, StatusCode::NOT_FOUND, "{scenario}: {id} must not exist: {:?}", retrieved.body);
    }
}

/// Clause 5.5.12: "A `datasetId` cannot be deleted by setting it to the value
/// `urn:ngsi-ld:null`." The merge stored the sentinel as a second instance's
/// `datasetId` and served it back, which clause 5.5.4 forbids as a first level
/// member value.
#[tokio::test]
async fn regression_null_sentinel_a_merge_cannot_delete_a_dataset_id() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:merge-dataset-id");

    create_entity(client, &building_with(&id, "temperature", &temperature_in_dataset_a())).await;

    let fragment = building_with(&id, "temperature", &json!({"type": "Property", "value": 9, "datasetId": NULL_URI}));
    let merged = client.merge_entity(&id, &fragment, None).await.expect("merge entity request");
    assert_bad_request_data(&merged, "clause 5.5.12 forbids deleting a datasetId with the sentinel");

    let stored = stored_entity(client, &id).await;
    assert_eq!(
        stored["temperature"]["datasetId"],
        json!(DATASET_A),
        "the refused merge must leave the instance identity untouched: {stored}"
    );
    assert_eq!(stored["temperature"]["value"], json!(21.5), "the refused merge must not be applied: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.8 closes the partial update algorithm with the same sentence:
/// "A `datasetId` cannot be deleted by setting it to the value
/// `urn:ngsi-ld:null`." It binds Update Attributes (clause 5.6.2) and Partial
/// Update Attribute (clause 5.6.4) alike.
#[tokio::test]
async fn regression_null_sentinel_a_partial_update_cannot_delete_a_dataset_id() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:partial-update-dataset-id");

    create_entity(client, &building_with(&id, "temperature", &temperature_in_dataset_a())).await;

    let fragment = json!({
        "temperature": {"type": "Property", "value": 9, "datasetId": NULL_URI},
        "@context": [CORE_CONTEXT]
    });
    let updated = client.update_entity_attrs(&id, &fragment).await.expect("update attributes request");
    assert_bad_request_data(&updated, "clause 5.6.2 cannot delete a datasetId with the sentinel");

    let attribute_fragment = json!({"type": "Property", "value": 9, "datasetId": NULL_URI, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&id, "temperature", &attribute_fragment)
        .await
        .expect("partial update attribute request");
    assert_bad_request_data(&patched, "clause 5.6.4 cannot delete a datasetId with the sentinel");

    let stored = stored_entity(client, &id).await;
    assert_eq!(
        stored["temperature"]["datasetId"],
        json!(DATASET_A),
        "neither refusal may change the instance identity: {stored}"
    );
    assert_eq!(stored["temperature"]["value"], json!(21.5), "neither refusal may be applied: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.7.4 opens Batch Entity Creation with "execute the behaviour
/// defined in clause 5.5.4 on JSON-LD validation", and clause 5.5.4 excepts
/// only the Fragments of a partial update or a merge. The batch endpoint
/// created verbatim what `POST /entities` refused.
#[tokio::test]
async fn regression_null_sentinel_batch_create_refuses_every_prohibited_form() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let (body, ids) = entities_carrying_every_prohibited_form(&scope, "batch-create");

    let response = client.batch_create(&body).await.expect("batch create request");
    assert_batch_refused_every_entity(&response, ids.len(), "clause 5.6.7.4 runs the clause 5.5.4 validation");
    assert_none_stored(client, &ids, "a refused batch create stores nothing").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8.4 runs the same validation, and creates the entity locally
/// "if it does not exist ... executing the behaviour defined by clause 5.6.1" —
/// so an upsert payload has to be valid as a creation, where a deletion
/// sentinel has nothing to delete.
#[tokio::test]
async fn regression_null_sentinel_batch_upsert_refuses_every_prohibited_form() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let (body, ids) = entities_carrying_every_prohibited_form(&scope, "batch-upsert");

    let response = client.batch_upsert(&body).await.expect("batch upsert request");
    assert_batch_refused_every_entity(&response, ids.len(), "clause 5.6.8.4 runs the clause 5.5.4 validation");
    assert_none_stored(client, &ids, "a refused batch upsert stores nothing").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.9.4's local phase runs "the behaviour defined by clause 5.6.3"
/// -- Append Attributes -- and counts a success as "(Attributes appended)",
/// where clause 5.6.20.4's merge phase counts "(Attributes updated, appended
/// or deleted)". Neither clause 5.6.9 nor clause 5.6.3 carries the sentence
/// "NGSI-LD Nulls should be supported by this operation" that clauses 5.6.2,
/// 5.6.4 and 5.6.17 carry, and clause 5.5.4 excepts only Fragments "used in
/// partial update and merge operations (as mandated by clause 5.5.8 and clause
/// 5.5.12)". A batch update therefore has no deletion semantics to give the
/// sentinel: admitting it stored the literal `urn:ngsi-ld:null` as an attribute
/// value, which the broker then served back to clients.
#[tokio::test]
async fn regression_null_sentinel_batch_update_refuses_the_deletion_sentinel() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:batch-update-null");

    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 21.5},
        "occupancy": {"type": "Property", "value": 12},
        "@context": [CORE_CONTEXT]
    });
    create_entity(client, &entity).await;

    for (attribute, node) in [("temperature", json!(NULL_URI)), ("occupancy", json!({"type": "Property", "value": NULL_URI}))] {
        let body = json!([building_with(&id, attribute, &node)]);
        let response = client.batch_update(&body).await.expect("batch update request");
        assert_batch_refused_every_entity(&response, 1, "clause 5.6.9.4 runs clause 5.6.3, which declares no null support");
    }

    // The attributes the refused requests named are untouched, so nothing was
    // half-applied and no sentinel reached storage.
    let read = client
        .get_entity(&id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("read the entity back");
    let body = read.json_value().expect("an entity body");
    assert_eq!(body["temperature"]["value"], json!(21.5), "{body}");
    assert_eq!(body["occupancy"]["value"], json!(12), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.20 is the batch form of Merge Entity, and clause 5.5.12 gives its
/// Fragments the sentinel "to indicate the removal of existing values within
/// the target member value" at any depth.
#[tokio::test]
async fn regression_null_sentinel_batch_merge_admits_the_deletion_sentinel() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:batch-merge-null");

    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 21.5},
        "address": address_node(),
        "@context": [CORE_CONTEXT]
    });
    create_entity(client, &entity).await;

    let body = json!([{
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": NULL_URI},
        "address": {"type": "Property", "value": {"street": "Pariser Platz", "country": NULL_URI}},
        "@context": [CORE_CONTEXT]
    }]);
    let response = client.batch_merge(&body).await.expect("batch merge request");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "clause 5.5.12 gives a merge fragment the sentinel at every depth: {:?}",
        response.body
    );

    let stored = stored_entity(client, &id).await;
    assert!(
        stored.get("temperature").is_none(),
        "the value-position sentinel deletes the attribute: {stored}"
    );
    assert_eq!(stored["address"]["value"]["street"], json!("Pariser Platz"), "{stored}");
    assert_eq!(
        stored["address"]["value"]["city"],
        json!("Berlin"),
        "an untouched key survives the merge: {stored}"
    );
    assert!(
        stored["address"]["value"].get("country").is_none(),
        "the nested sentinel deletes its key: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.4: "Any attempt to use `urn:ngsi-ld:null` as the value of a key
/// value pair within a JSON object, which is the right-hand side of the value
/// of a Property, with the exception of NGSI-LD Fragments used in merge
/// operations (see clause 5.5.12), shall result in an error of type
/// `BadRequestData`." A creation is not a merge fragment.
#[tokio::test]
async fn regression_null_sentinel_a_nested_null_is_refused_on_create() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:create-nested-null");

    let entity = building_with(&id, "address", &json!({"type": "Property", "value": {"country": NULL_URI}}));
    let response = client.post_entity(&entity).send().await.expect("create entity request");
    assert_bad_request_data(&response, "clause 5.5.4 excepts merge fragments alone from the nested rule");
    assert_none_stored(client, &[id], "a refused creation stores nothing").await;

    scope.finish().await.expect("cleanup failed");
}

/// The same prohibition on the write paths that are not fragments either:
/// Append Attributes (clause 5.6.3), Replace Attribute (clause 5.6.19) and
/// Replace Entity (clause 5.6.18, whose clause 5.6.18.4 states that Replace
/// "does not support" null values).
#[tokio::test]
async fn regression_null_sentinel_a_nested_null_is_refused_on_append_and_replace() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:append-replace-nested-null");

    create_entity(client, &building_with(&id, "address", &address_node())).await;

    let appended = client
        .append_entity_attrs(
            &id,
            &json!({"office": {"type": "Property", "value": {"country": NULL_URI}}, "@context": [CORE_CONTEXT]}),
        )
        .await
        .expect("append attributes request");
    assert_bad_request_data(&appended, "clause 5.6.3 is not a fragment operation");

    let replaced_attribute = client
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{}/attrs/address", encode(&id)))
        .header("Content-Type", "application/ld+json")
        .json(&json!({"type": "Property", "value": {"country": NULL_URI}, "@context": [CORE_CONTEXT]}))
        .send()
        .await
        .expect("replace attribute request");
    assert_bad_request_data(&replaced_attribute, "clause 5.6.19 is not a fragment operation");

    let replacement = building_with(&id, "address", &json!({"type": "Property", "value": {"country": NULL_URI}}));
    let replaced_entity = client
        .replace_entity(&id, &replacement, "application/ld+json", None)
        .await
        .expect("replace entity request");
    assert_bad_request_data(&replaced_entity, "clause 5.6.18.4 states Replace does not support nulls");

    let stored = stored_entity(client, &id).await;
    assert_eq!(
        stored["address"]["value"]["country"],
        json!("Germany"),
        "no refused write may reach the store: {stored}"
    );
    assert!(stored.get("office").is_none(), "the refused append must not create an attribute: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12 EXAMPLE 2: merging `{"country": "urn:ngsi-ld:null"}` into an
/// `address` Property value "updat[es] street and remov[es] country", leaving
/// `city` standing. This is the one operation clause 5.5.4 excepts from the
/// nested prohibition.
#[tokio::test]
async fn regression_null_sentinel_a_nested_null_is_admitted_by_merge() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:merge-nested-null");

    create_entity(client, &building_with(&id, "address", &address_node())).await;

    let fragment = building_with(
        &id,
        "address",
        &json!({"type": "Property", "value": {"street": "Pariser Platz", "country": NULL_URI}}),
    );
    let merged = client.merge_entity(&id, &fragment, None).await.expect("merge entity request");
    assert_eq!(
        merged.status,
        StatusCode::NO_CONTENT,
        "clause 5.5.12 EXAMPLE 2 merges a nested sentinel: {:?}",
        merged.body
    );

    let stored = stored_entity(client, &id).await;
    assert_eq!(stored["address"]["value"]["street"], json!("Pariser Platz"), "{stored}");
    assert_eq!(stored["address"]["value"]["city"], json!("Berlin"), "an untouched key survives: {stored}");
    assert!(
        stored["address"]["value"].get("country").is_none(),
        "the nested sentinel removes country: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.4's value-position prohibition names the `languageMap` spelling
/// `{"@none": "urn:ngsi-ld:null"}` explicitly, and clauses 4.5.21.2 and 4.5.22.2
/// give the two list families theirs: the array holding one single NGSI-LD Null
/// of clause 4.5.0, written into `valueList` or `objectList`. A creation admits
/// none of them.
#[tokio::test]
async fn regression_null_sentinel_every_family_spelling_is_refused_on_create() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let cases = [
        (
            "language-map",
            "label",
            json!({"type": "LanguageProperty", "languageMap": {"@none": NULL_URI}}),
            "clause 4.5.18 spells a LanguageProperty deletion as {\"@none\": null}",
        ),
        (
            "value-list",
            "tags",
            json!({"type": "ListProperty", "valueList": [NULL_URI]}),
            "clause 4.5.21.2 spells a ListProperty deletion as a single-null valueList",
        ),
        (
            "object-list",
            "hasRooms",
            json!({"type": "ListRelationship", "objectList": [NULL_URI]}),
            "clause 4.5.22.2 spells a ListRelationship deletion as a single-null objectList",
        ),
        (
            "object-list-normalized",
            "hasRooms",
            json!({"type": "ListRelationship", "objectList": [{"object": NULL_URI}]}),
            "clause 4.5.22.2 makes each normalized objectList element an object with a single object member",
        ),
        (
            "relationship-object",
            "belongsTo",
            json!({"type": "Relationship", "object": NULL_URI}),
            "clause 5.5.4 forbids the sentinel as the right-hand side of object",
        ),
    ];

    for (local, attribute, node, scenario) in cases {
        let id = scope.id(&format!("Building:create-{local}"));
        let response = client
            .post_entity(&building_with(&id, attribute, &node))
            .send()
            .await
            .expect("create entity request");
        assert_bad_request_data(&response, scenario);
        assert_none_stored(client, &[id], scenario).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// The path a forwarded create lands on at a registered Context Source.
const FORWARDED_CREATE_PATH: &str = "/ngsi-ld/v1/entities";

/// An exclusive registration claiming one named Property of the entities it
/// names, so a create carrying only that Property has its whole payload
/// claimed by the source and leaves the local phase nothing to do.
///
/// Clause 5.9.2 lets a registration narrow to `entities` and `propertyNames`,
/// and this broker requires both of the exclusive mode: clause 4.3.6.3 makes
/// an exclusive registration the sole holder of what it names, so what it
/// names has to be stated exactly.
fn exclusive_registration(id: &str, endpoint: &str, entity_type: &str, covered: &[&str], property: &str) -> Value {
    let entities: Vec<Value> = covered.iter().map(|entity| json!({"id": entity, "type": entity_type})).collect();
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": "exclusive",
        "operations": ["createEntity"],
        "information": [{"entities": entities, "propertyNames": [property]}],
        "@context": [CORE_CONTEXT]
    })
}

/// Clause 5.6.1.4 lists "Execute the behaviour defined in clause 5.5.4 on
/// JSON-LD validation" as its first bullet, and the exclusive, redirect and
/// inclusive forwarding bullets follow it. Two things follow from that order,
/// and the broker did neither: it forwarded a payload it was going to refuse,
/// and -- when an exclusive registration claimed every attribute, leaving the
/// local phase nothing to do -- it answered 201 having run no validation at
/// all. The control at the end proves the registration really does claim this
/// entity, so the "nothing was forwarded" assertion is not vacuous.
#[tokio::test]
async fn regression_null_sentinel_a_fully_proxied_create_is_refused_before_forwarding() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let entity_type = "NullSentinelProxiedProbe";

    let refused = scope.id("Building:proxied-null");
    let forwarded = scope.id("Building:proxied-ok");
    let registration = exclusive_registration(
        &scope.id("registration:null-sentinel-exclusive"),
        &source.url("/"),
        entity_type,
        &[&refused, &forwarded],
        "temperature",
    );
    let created = client.create_registration(&registration).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    source.set_stub(Method::POST, FORWARDED_CREATE_PATH, 201).await;
    let response = client
        .post_entity(&json!({
            "id": refused,
            "type": entity_type,
            "temperature": {"type": "Property", "value": NULL_URI},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await
        .expect("create entity request");

    assert_bad_request_data(&response, "clause 5.5.4 runs before the forwarding bullets");
    assert_eq!(source.get_total_count(), 0, "a payload clause 5.5.4 refuses reaches no Context Source");
    assert_none_stored(client, &[refused], "a refused create stores nothing").await;

    // The control: the same registration claims a well-formed create of the
    // sibling entity, so the source is genuinely on this path.
    let accepted = client
        .post_entity(&json!({
            "id": forwarded,
            "type": entity_type,
            "temperature": {"type": "Property", "value": 21.5},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await
        .expect("create entity request");
    assert!(
        accepted.status.is_success(),
        "a well-formed create of a proxied type succeeds: {:?}",
        accepted.body
    );
    assert_eq!(
        source.get_count(Method::POST, FORWARDED_CREATE_PATH),
        1,
        "the exclusive registration claims this entity, so the refusal above was a refusal and not a miss"
    );

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8.4: "If there were an existing Entity with the same Entity ID,
/// the behaviour defined by clause 5.6.2 shall be executed, but limited to a
/// local operation, if the requested update mode is 'update'." Clause 5.6.2.4
/// is one of the three clauses that carry "NGSI-LD Nulls should be supported
/// by this operation", so the update mode of an upsert deletes with the
/// sentinel rather than refusing it. An Entity that does not exist is created
/// instead "executing the behaviour defined by clause 5.6.1", which supports no
/// NGSI-LD Null, and clause 5.2.1 answers `BadRequestData` "in all other
/// cases": the same payload refuses that entity alone.
#[tokio::test]
async fn regression_null_sentinel_batch_upsert_update_mode_deletes_with_the_sentinel_and_refuses_it_on_create() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let existing = scope.id("Building:upsert-update-existing");
    let fresh = scope.id("Building:upsert-update-fresh");

    create_entity(
        client,
        &json!({
            "id": existing,
            "type": "Building",
            "temperature": {"type": "Property", "value": 21.5},
            "humidity": {"type": "Property", "value": 40},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let body = json!([
        {
            "id": existing,
            "type": "Building",
            "temperature": {"type": "Property", "value": NULL_URI},
            "humidity": {"type": "Property", "value": 55},
            "@context": [CORE_CONTEXT]
        },
        {
            "id": fresh,
            "type": "Building",
            "temperature": {"type": "Property", "value": NULL_URI},
            "humidity": {"type": "Property", "value": 60},
            "@context": [CORE_CONTEXT]
        }
    ]);
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/upsert?options=update")
        .header("content-type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("batch upsert request");
    // Clause 5.6.8.5: one entity updated and one refused is a partial success.
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "{:?}", response.body);
    let result = response.json_value().expect("a partly failed batch carries a result body");
    assert_eq!(result["success"], json!([existing]), "clause 5.6.2.4 supports NGSI-LD Nulls: {result}");
    let errors = result["errors"].as_array().expect("a batch result carries an errors array");
    assert_eq!(errors.len(), 1, "only the entity to create is refused: {result}");
    assert_eq!(errors[0]["entityId"], json!(fresh), "{result}");
    assert_eq!(
        errors[0]["error"]["type"],
        json!(BAD_REQUEST_DATA),
        "clause 5.6.1 supports no NGSI-LD Null: {result}"
    );

    let updated = stored_entity(client, &existing).await;
    assert_eq!(updated["temperature"], Value::Null, "the sentinel deleted the attribute: {updated}");
    assert_eq!(updated["humidity"]["value"], json!(55), "the sibling attribute was updated: {updated}");

    assert_none_stored(client, &[fresh], "the refused entity is not created").await;

    scope.finish().await.expect("cleanup failed");
}

/// The create branch of an update-mode upsert refuses an NGSI-LD Null wherever
/// it stands, not only as a whole member: clause 5.6.1 supports none, and
/// clause 5.2.1 answers `BadRequestData` for every Null outside a partial update
/// or merge. The broker dropped a Null whole member and refused the rest, so
/// what a create did with a Null depended on how deep it was written.
#[tokio::test]
async fn regression_null_sentinel_batch_upsert_update_mode_refuses_a_null_at_any_depth_on_create() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    // Each shape is the member that carries the Null, written beside the
    // entity's identifier and type.
    let shapes = [
        ("whole-member", "speed", json!(NULL_URI)),
        ("unit-code", "speed", json!({"type": "Property", "value": 5, "unitCode": NULL_URI})),
        ("observed-at", "speed", json!({"type": "Property", "value": 5, "observedAt": NULL_URI})),
        ("sub-attribute", "speed", json!({"type": "Property", "value": 5, "accuracy": NULL_URI})),
        (
            "dataset-instance",
            "speed",
            json!([
                {"type": "Property", "value": NULL_URI, "datasetId": DATASET_A},
                {"type": "Property", "value": 5}
            ]),
        ),
        ("scope", "scope", json!(NULL_URI)),
    ];
    let ids: Vec<String> = shapes.iter().map(|(local, ..)| scope.id(&format!("Building:upsert-create-{local}"))).collect();
    let body = Value::Array(shapes.iter().zip(&ids).map(|((_, member, node), id)| building_with(id, member, node)).collect());

    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/upsert?options=update")
        .header("content-type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("batch upsert request");
    assert_batch_refused_every_entity(&response, ids.len(), "clause 5.6.1 supports no NGSI-LD Null");
    assert_none_stored(client, &ids, "a refused create stores nothing").await;

    scope.finish().await.expect("cleanup failed");
}

/// The replace mode of the same operation admits none of it: clause 5.6.8.4
/// has an existing Entity "completely replaced by the new Entity content
/// provided", which is the Replace Entity of clause 5.6.18, and clause
/// 5.6.18.4 states that NGSI-LD Nulls are not supported by it.
#[tokio::test]
async fn regression_null_sentinel_batch_upsert_replace_mode_still_refuses_the_sentinel() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:upsert-replace-null");

    let body = json!([{
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": NULL_URI},
        "@context": [CORE_CONTEXT]
    }]);
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/upsert?options=replace")
        .header("content-type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("batch upsert request");
    assert_batch_refused_every_entity(&response, 1, "clause 5.6.18.4 does not support NGSI-LD Nulls");
    assert_none_stored(client, &[id], "a refused upsert stores nothing").await;

    scope.finish().await.expect("cleanup failed");
}

/// The members that hold the Null beside data: neither the deletion encoding
/// of clause 4.5.0 ("an array with one single NGSI-LD Null") nor data, since
/// "null cannot be used as a value in NGSI-LD". The broker created such an
/// entity with 201 and then could not read it back: every retrieval answered
/// an internal error, because the stored-document decoder refuses the Null
/// beside data.
fn nodes_holding_the_null_beside_data() -> [(&'static str, Value); 5] {
    [
        ("tags", json!({"type": "Property", "value": ["a", NULL_URI]})),
        ("owners", json!({"type": "Relationship", "object": ["urn:ngsi-ld:Person:a", NULL_URI]})),
        ("labels", json!({"type": "ListProperty", "valueList": ["a", NULL_URI]})),
        ("rooms", json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:a", NULL_URI]})),
        ("address", json!({"type": "Property", "value": {"lines": ["Pariser Platz", NULL_URI]}})),
    ]
}

/// Clause 5.5.4 refuses the Null wherever no deletion is meant, with
/// `BadRequestData`; beside data no write means a deletion, so creation through
/// the Core and the Temporal API refuses it and stores nothing.
#[tokio::test]
async fn regression_null_sentinel_beside_data_is_refused_on_create() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (attribute, node) in nodes_holding_the_null_beside_data() {
        let id = scope.id(&format!("Building:beside-data-{attribute}"));
        let created = client
            .post_entity(&building_with(&id, attribute, &node))
            .send()
            .await
            .expect("create entity request");
        assert_bad_request_data(&created, &format!("create, {attribute}"));

        let temporal = json!({"id": id, "type": "Building", attribute: [node], "@context": [CORE_CONTEXT]});
        let created = client.create_temporal_entity(&temporal).await.expect("create temporal entity request");
        assert_bad_request_data(&created, &format!("temporal create, {attribute}"));

        assert_none_stored(client, &[id], &format!("a refused create of {attribute} stores nothing")).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12 gives a merge the Null as the whole value of a member at any
/// depth, never beside data. A merge carrying it beside data is refused, and
/// the entity it was aimed at stays readable and unchanged.
#[tokio::test]
async fn regression_null_sentinel_beside_data_is_refused_by_merge_and_update() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:beside-data-merge");

    create_entity(client, &building_with(&id, "address", &address_node())).await;

    for (attribute, node) in nodes_holding_the_null_beside_data() {
        let fragment = building_with(&id, attribute, &node);
        let merged = client.merge_entity(&id, &fragment, None).await.expect("merge entity request");
        assert_bad_request_data(&merged, &format!("merge, {attribute}"));

        let update = json!({attribute: node, "@context": [CORE_CONTEXT]});
        let updated = client.update_entity_attrs(&id, &update).await.expect("update attributes request");
        assert_bad_request_data(&updated, &format!("update, {attribute}"));
    }

    let stored = stored_entity(client, &id).await;
    assert_eq!(stored["address"], address_node(), "no refused write may reach the store: {stored}");
    for (attribute, _) in nodes_holding_the_null_beside_data() {
        if attribute != "address" {
            assert!(stored.get(attribute).is_none(), "the refused writes must not create {attribute}: {stored}");
        }
    }

    scope.finish().await.expect("cleanup failed");
}
