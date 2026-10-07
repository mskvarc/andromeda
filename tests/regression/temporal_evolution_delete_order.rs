//! Writing to an Entity after the Temporal Evolution is already gone.
//!
//! ETSI GS CIM 009 v1.9.1 gives a client two independent deletions of one
//! Entity. Clause 5.6.6.4 removes current state — "The input data shall be used
//! to remove the entity locally if it exists" — and clause 5.6.16.4 removes
//! history — "If the target Temporal Evolution of an Entity exists locally, the
//! entire Temporal Evolution of the Entity shall be removed". Neither clause
//! mentions the other, so an Entity whose history a client already deleted is an
//! ordinary state of the system that every operation has to survive.
//!
//! Two things follow. A deletion of current state records one clause 4.5.7/4.5.8
//! tombstone per attribute instance the Entity still held, and the temporal
//! catalogue has to accept a tombstone whose Temporal Evolution is missing,
//! because a write that states no Entity type cannot create one (clause 4.5.1).
//! The same holds for every write that observes something — clauses 5.6.2,
//! 5.6.3, 5.6.4, 5.6.17 and 5.6.19 — whose Entity types come from the Entity the
//! write produced, not from the request Fragment, whose `type` those clauses
//! each make optional. A refusal in either place would reach the client as the
//! `InternalError` clause 5.5.6 reserves for "an unexpected error [that] happens
//! and the operation cannot be completed" — which a state the API itself lets a
//! client produce is not, and which none of the clause 6 response tables for
//! those operations even lists.
//!
//! The two halves resolve differently and both directions are pinned here. An
//! observation states what the Entity is, so it recreates the evolution clause
//! 5.6.16 removed; a deletion states only that instances ended, and clauses
//! 4.5.7 and 4.5.8 make an ending "an instance of the Property [...] recorded
//! with its value set to the URI urn:ngsi-ld:null and the deletedAt Temporal
//! Property set" — one more instance of a sequence that is no longer there, so
//! it records nothing and brings nothing back.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{entity_attribute::DatasetSelection, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.5.2 gives a referred resource that is not there.
const RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";

/// One `Building` carrying two ordinary Properties, so its deletion records
/// more than one clause 4.5.7 tombstone.
fn building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 21.5},
        "category": {"type": "VocabProperty", "vocab": "office"},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create `id` as a two-Property `Building` and assert clause 5.6.1 accepted it.
async fn create_building(client: &TestClient, id: &str) {
    let created = client.post_entity(&building(id)).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create {id}: {:?}", created.body);
}

/// Delete the Temporal Evolution of `id` and assert clause 5.6.16.5 answered
/// with no content, so the Entity now stands without any history.
async fn delete_temporal_evolution(client: &TestClient, id: &str) {
    let deleted = client.delete_temporal_entity(id).await.expect("delete temporal evolution request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete temporal evolution of {id}: {:?}", deleted.body);

    let retrieved = client
        .get_temporal_entity(id, "", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(
        retrieved.status,
        StatusCode::NOT_FOUND,
        "clause 5.6.16.4 removes the entire Temporal Evolution: {:?}",
        retrieved.body
    );
}

/// Assert `response` is the clause 5.5.2 `ResourceNotFound` of a target the
/// broker does not know, rather than a success.
fn assert_resource_not_found(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND), "{scenario}: {problem}");
}

/// Assert `id` no longer answers clause 5.6.5 Retrieve Entity.
async fn assert_entity_gone(client: &TestClient, id: &str) {
    let retrieved = client
        .get_entity(id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve entity request");
    assert_eq!(
        retrieved.status,
        StatusCode::NOT_FOUND,
        "clause 5.6.6.4 removes the entity locally: {:?}",
        retrieved.body
    );
}

/// Clause 5.6.6.4: "The input data shall be used to remove the entity locally
/// if it exists." The Entity exists, so it is removed and clause 5.6.6.5
/// answers with no output data. Its history having been deleted first by clause
/// 5.6.16 leaves the tombstones of clauses 4.5.7 and 4.5.8 nothing to record,
/// which is not one of the two errors clause 5.6.6.4 raises and is certainly not
/// the "unexpected error" of clause 5.5.6.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_an_entity_delete_after_its_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:delete-after-temporal-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let deleted = client.delete_entity(&id).await.expect("delete entity request");
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "clause 5.6.6 removes an entity whose Temporal Evolution clause 5.6.16 already removed: {:?}",
        deleted.body
    );
    assert_entity_gone(client, &id).await;

    scope.finish().await.expect("cleanup failed");
}

/// The other order: clause 5.6.6
/// removes current state while clause 5.6.16 leaves the Temporal Evolution
/// standing, so the evolution is still there to be deleted afterwards.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_history_delete_after_the_entity_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:temporal-delete-after-delete");

    create_building(client, &id).await;

    let deleted = client.delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete entity: {:?}", deleted.body);
    assert_entity_gone(client, &id).await;

    let deleted_history = client.delete_temporal_entity(&id).await.expect("delete temporal evolution request");
    assert_eq!(
        deleted_history.status,
        StatusCode::NO_CONTENT,
        "clause 5.6.16 removes a Temporal Evolution that outlived its current state: {:?}",
        deleted_history.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// An Entity that holds no Attribute has no instance for clause 4.5.7 to end,
/// so its deletion records no tombstone at all. Clause 5.6.6.4 removes it just
/// the same, whether or not a Temporal Evolution was ever there to remove.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_an_entity_that_never_recorded_an_instance_is_deleted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:attributeless");

    let entity = json!({"id": id, "type": "Building", "@context": [TEST_SUITE_CONTEXT]});
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create {id}: {:?}", created.body);

    let deleted_history = client.delete_temporal_entity(&id).await.expect("delete temporal evolution request");
    assert_eq!(
        deleted_history.status,
        StatusCode::NO_CONTENT,
        "delete temporal evolution: {:?}",
        deleted_history.body
    );

    let deleted = client.delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "clause 5.6.6 removes the entity: {:?}", deleted.body);
    assert_entity_gone(client, &id).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.10.4 runs Delete Entity "for each of the Entity ids included in
/// the input Array", so every identifier carries the behaviour of clause 5.6.6
/// and none of them may report a broker malfunction for a history a client
/// deleted first. Clause 5.6.10.5 answers a batch that refused nothing with no
/// content.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_batch_delete_after_the_histories_were_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = [scope.id("Building:batch-delete-a"), scope.id("Building:batch-delete-b")];

    for id in &ids {
        create_building(client, id).await;
        delete_temporal_evolution(client, id).await;
    }

    let body = Value::Array(ids.iter().map(|id| json!(id)).collect());
    let deleted = client.batch_delete(&body).await.expect("batch delete request");
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "clause 5.6.10 deletes every identifier whose Temporal Evolution clause 5.6.16 already removed: {:?}",
        deleted.body
    );
    for id in &ids {
        assert_entity_gone(client, id).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.5 Delete Attribute removes one Attribute from an Entity and
/// records the same clause 4.5.7 ending as an Entity deletion does, so it meets
/// a missing Temporal Evolution the same way and has just as little to record.
/// The Entity itself survives, so the Attribute that was not named survives too.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_an_attribute_delete_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:attribute-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let deleted = client
        .delete_entity_attr(&id, "temperature", None, DatasetSelection::Default, Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("delete attribute request");
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "clause 5.6.5 removes the attribute although clause 5.6.16 already removed the history: {:?}",
        deleted.body
    );

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve entity request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("the retrieved entity is JSON");
    assert!(body.get("temperature").is_none(), "clause 5.6.5 removed the named attribute: {body}");
    assert!(body.get("category").is_some(), "clause 5.6.5 removes only the named attribute: {body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.21 Purge Attributes drops the named Attributes from every Entity
/// a query matches, recording the same clause 4.5.7 endings one at a time. A
/// purge whose targets have no Temporal Evolution left has nothing to record and
/// still purges.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_purge_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:purge");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let query = format!("id={}&drop=temperature&local=true", urlencoding::encode(&id));
    let purged = client
        .delete_entities_by_query(&query, Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("purge attributes request");
    assert_eq!(
        purged.status,
        StatusCode::NO_CONTENT,
        "clause 5.6.21 purges an attribute although clause 5.6.16 already removed the history: {:?}",
        purged.body
    );

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve entity request");
    assert_eq!(
        retrieved.status,
        StatusCode::OK,
        "clause 5.6.21 purges attributes, not entities: {:?}",
        retrieved.body
    );
    let body = retrieved.json_value().expect("the retrieved entity is JSON");
    assert!(body.get("temperature").is_none(), "clause 5.6.21 dropped the named attribute: {body}");
    assert!(body.get("category").is_some(), "clause 5.6.21 drops only the named attributes: {body}");

    scope.finish().await.expect("cleanup failed");
}

/// The control. Clause 5.6.6.4 still raises `ResourceNotFound` when "there is no
/// existing Entity whose id (URI) ... is equivalent held locally", so a deletion
/// that finds nothing must keep answering 404. Turning the missing Temporal
/// Evolution into a success at the HTTP boundary would turn this into a 204 as
/// well, reporting a deletion that never happened.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_delete_of_an_unknown_entity_is_still_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:never-created");

    let refused = client.delete_entity(&id).await.expect("delete entity request");
    assert_resource_not_found(&refused, "clause 5.6.6.4 raises ResourceNotFound for an entity it does not know");

    scope.finish().await.expect("cleanup failed");
}

/// The same control one step later: an Entity deleted while its Temporal
/// Evolution was already gone is genuinely gone, so deleting it a second time
/// meets clause 5.6.6.4's `ResourceNotFound` rather than another 204.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_repeated_entity_delete_is_still_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:deleted-twice");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let deleted = client.delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "first delete: {:?}", deleted.body);

    let refused = client.delete_entity(&id).await.expect("delete entity request");
    assert_resource_not_found(&refused, "clause 5.6.6.4 raises ResourceNotFound for an entity already deleted");

    scope.finish().await.expect("cleanup failed");
}

/// The attribute-shaped control. Clause 5.6.5.4 raises `ResourceNotFound` when
/// the Entity holds no such Attribute, and a missing Temporal Evolution must not
/// mask that: the Entity is there, the Attribute is not, and the request is
/// still a 404.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_delete_of_an_unknown_attribute_is_still_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:unknown-attribute");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let refused = client
        .delete_entity_attr(&id, "humidity", None, DatasetSelection::Default, Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("delete attribute request");
    assert_resource_not_found(&refused, "clause 5.6.5.4 raises ResourceNotFound for an attribute the entity does not hold");

    scope.finish().await.expect("cleanup failed");
}

/// The batch-shaped control. Clause 5.6.10.4 reports one `BatchEntityError` per
/// identifier it could not delete, so an unknown identifier must still be
/// reported as `ResourceNotFound` in the clause 5.2.16 result beside the
/// identifier the batch did delete.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_batch_delete_still_reports_an_unknown_identifier() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let present = scope.id("Building:batch-control-present");
    let absent = scope.id("Building:batch-control-absent");

    create_building(client, &present).await;
    delete_temporal_evolution(client, &present).await;

    let body = json!([present, absent]);
    let response = client.batch_delete(&body).await.expect("batch delete request");
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "batch delete: {:?}", response.body);
    let result = response.json_value().expect("a batch answers with a clause 5.2.16 result");
    let errors = result["errors"].as_array().expect("the clause 5.2.16 result lists its errors");
    let error = errors.iter().find(|error| error["entityId"] == json!(absent)).unwrap_or(&Value::Null);
    assert_eq!(
        error["error"]["type"],
        json!(RESOURCE_NOT_FOUND),
        "clause 5.6.10.4 reports the identifier it could not delete: {result}"
    );
    assert!(
        !errors.iter().any(|error| error["entityId"] == json!(present)),
        "clause 5.6.10.4 deletes the identifier whose Temporal Evolution was already removed: {result}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Retrieve `id` as clause 5.6.5 Retrieve Entity and return the body, asserting
/// the entity is still there to be read.
async fn stored_entity(client: &TestClient, id: &str) -> Value {
    let retrieved = client
        .get_entity(id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve entity request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve {id}: {:?}", retrieved.body);
    retrieved.json_value().expect("the retrieved entity is JSON")
}

/// Retrieve the whole Temporal Evolution of `id` (clause 5.7.3) and return the
/// clause 5.2.20 `EntityTemporal` body.
///
/// No temporal query is supplied. Clause 5.2.21 makes `observedAt` the default
/// `timeproperty`, and clause 4.11 positions an instance on a Temporal Property
/// only when the instance carries one, so a window would exclude every instance
/// whose Context Producer declared no `observedAt` — which is all of them here.
async fn stored_temporal_evolution(client: &TestClient, id: &str) -> Value {
    let retrieved = client
        .get_temporal_entity(id, "", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(
        retrieved.status,
        StatusCode::OK,
        "clause 5.7.3 answers with the Temporal Evolution of {id}: {:?}",
        retrieved.body
    );
    retrieved.json_value().expect("the retrieved Temporal Evolution is JSON")
}

/// Assert a write answered with the 204 its clause 6 table gives it.
///
/// Tables 6.6.3.1-2 (Append Attributes), 6.6.3.2-2 (Update Attributes),
/// 6.5.3.4-2 (Merge Entity), 6.7.3.1-2 (Partial Attribute Update) and
/// 6.7.3.3-2 (Replace Attribute) each list 204, 207, 400 and 404 and no other
/// code at all — 500 is not among the responses any of these operations may
/// give, and clause 5.5.6 reserves `InternalError` for "an unexpected error
/// [that] happens and the operation cannot be completed", which a state
/// clauses 5.6.6 and 5.6.16 between them let a client reach is not.
fn assert_write_succeeded(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{scenario}: {:?}", response.body);
}

/// Clause 5.6.3.4 appends the Fragment's Attributes to a target Entity that
/// "is equivalent held locally", and lists exactly two errors — `BadRequestData`
/// for a malformed id and `ResourceNotFound` for an Entity the endpoint does not
/// know. The Entity is known, so the append lands; that clause 5.6.16 removed
/// its Temporal Evolution first bears on nothing clause 5.6.3.4 says, because
/// clause 4.5.6 lets the write state the Entity's clause 4.5.1 types and
/// recreate the evolution it extends.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_an_append_attributes_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:append-after-temporal-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let appended = client
        .append_entity_attrs(&id, &json!({"humidity": {"type": "Property", "value": 44}, "@context": [TEST_SUITE_CONTEXT]}))
        .await
        .expect("append attributes request");
    assert_write_succeeded(&appended, "clause 5.6.3 appends although clause 5.6.16 already removed the history");

    let body = stored_entity(client, &id).await;
    assert_eq!(body["humidity"]["value"], json!(44), "clause 5.6.3.4 appended the Attribute: {body}");
    assert_eq!(
        body["temperature"]["value"],
        json!(21.5),
        "clause 5.6.3.4 leaves the other Attributes alone: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The observable half of "created anew". Clause 5.6.16.4 says only that "the
/// entire Temporal Evolution of the Entity shall be removed" — it reserves
/// nothing, tombstones no identifier and forbids no later write — and clause
/// 5.7.3.4 reads a missing evolution as an empty one ("S is empty in case no
/// Temporal Evolution of the Entity is [available]") rather than as a
/// permanently closed door. Clauses 4.5.7 and 4.5.8 then make each Attribute's
/// evolution "the sequence of instances of the referred Property during a period
/// of time within its lifetime", so the next write starts that sequence again,
/// and clause 4.5.6 makes the recreated evolution carry the Entity's own type.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_write_after_the_history_was_deleted_records_a_new_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:history-recreated");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let appended = client
        .append_entity_attrs(&id, &json!({"humidity": {"type": "Property", "value": 44}, "@context": [TEST_SUITE_CONTEXT]}))
        .await
        .expect("append attributes request");
    assert_write_succeeded(&appended, "clause 5.6.3 appends after the history was deleted");

    let evolution = stored_temporal_evolution(client, &id).await;
    assert_eq!(
        evolution["type"],
        json!("Building"),
        "clause 4.5.6 makes the Temporal Evolution the Entity as mandated by clause 4.5.1: {evolution}"
    );
    let instances = evolution["humidity"].as_array().expect("clause 4.5.7 renders the instances as an Array");
    assert_eq!(instances.len(), 1, "the recreated evolution holds only what was written after it: {evolution}");
    assert_eq!(
        instances[0]["value"],
        json!(44),
        "clause 4.5.7 records the instance the append observed: {evolution}"
    );
    assert!(
        evolution.get("temperature").is_none(),
        "clause 5.6.16.4 removed the entire evolution, so nothing written before it comes back: {evolution}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.2.4 Update Attributes performs "a partial update patch operation
/// over the remains of the target Entity", replacing the instances the Fragment
/// names. Table 6.6.3.2-2 gives it 204 when "All the Attributes were updated
/// successfully" and no 500 at all.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_an_update_attributes_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:update-after-temporal-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let updated = client
        .update_entity_attrs(
            &id,
            &json!({"temperature": {"type": "Property", "value": 30.5}, "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("update attributes request");
    assert_write_succeeded(&updated, "clause 5.6.2 updates although clause 5.6.16 already removed the history");

    let body = stored_entity(client, &id).await;
    assert_eq!(
        body["temperature"]["value"],
        json!(30.5),
        "clause 5.6.2.4 replaced the existing instance: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.17.4 Merge Entity performs "a merge operation over the target
/// Entity as mandated by clause 5.5.12" and, like clauses 5.6.2.4 and 5.6.3.4,
/// raises only `BadRequestData` and `ResourceNotFound`. Table 6.5.3.4-2 answers
/// 204 when "All the Attributes were merged successfully".
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_merge_entity_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:merge-after-temporal-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let merged = client
        .merge_entity(
            &id,
            &json!({"temperature": {"type": "Property", "value": 18.25}, "@context": [TEST_SUITE_CONTEXT]}),
            None,
        )
        .await
        .expect("merge entity request");
    assert_write_succeeded(&merged, "clause 5.6.17 merges although clause 5.6.16 already removed the history");

    let body = stored_entity(client, &id).await;
    assert_eq!(
        body["temperature"]["value"],
        json!(18.25),
        "clause 5.6.17.4 merged the supplied Attribute: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A merge whose Fragment carries `scope` and nothing else. Clause 5.6.17.4
/// says "If scope is included in the Fragment and overwrite is allowed, the
/// scope of the target Entity will become the one included in the Fragment", and
/// clause 4.5.6 makes the Scope of a Temporal Evolution "a temporal
/// representation of a Property", so the merge still has an evolution to extend
/// and still has to be able to bring one into being.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_scope_only_merge_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:scope-merge-after-temporal-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let merged = client
        .merge_entity(&id, &json!({"scope": "/Madrid/Gardens", "@context": [TEST_SUITE_CONTEXT]}), None)
        .await
        .expect("merge entity request");
    assert_write_succeeded(&merged, "clause 5.6.17 merges a scope although clause 5.6.16 already removed the history");

    let body = stored_entity(client, &id).await;
    assert_eq!(body["scope"], json!("/Madrid/Gardens"), "clause 5.6.17.4 replaced the Entity's scope: {body}");
    assert_eq!(body["temperature"]["value"], json!(21.5), "a scope merge changes no Attribute: {body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.4 Partial Attribute update "only changes the elements provided in
/// an Entity Fragment, leaving the rest as they are". Table 6.7.3.1-2 gives it
/// 204, 400, 404 and 409 and no 500, so a Temporal Evolution clause 5.6.16
/// removed cannot make it one.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_partial_attribute_update_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:partial-update-after-temporal-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let updated = client
        .partial_update_attr(&id, "temperature", &json!({"value": 12.5, "@context": [TEST_SUITE_CONTEXT]}))
        .await
        .expect("partial attribute update request");
    assert_write_succeeded(&updated, "clause 5.6.4 updates although clause 5.6.16 already removed the history");

    let body = stored_entity(client, &id).await;
    assert_eq!(body["temperature"]["value"], json!(12.5), "clause 5.6.4.4 changed the element provided: {body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.19 Replace Attribute replaces one Attribute wholesale. Table
/// 6.7.3.3-2 lists 204, 400 and 404 for it and, as everywhere else here, no 500.
///
/// The Temporal Evolution is also checked, because the types a write states about
/// the Entity are what the catalogue records the evolution under: reading them
/// off a bare Attribute Fragment instead would take the `Property` of clause
/// 5.2.5 for an Entity type and overwrite the Entity's own.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_replace_attribute_after_the_history_was_deleted_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:replace-attribute-after-temporal-delete");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let replaced = client
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{}/attrs/temperature", urlencoding::encode(&id)))
        .header("Content-Type", "application/ld+json")
        .json(&json!({"type": "Property", "value": 5.5, "@context": [TEST_SUITE_CONTEXT]}))
        .send()
        .await
        .expect("replace attribute request");
    assert_write_succeeded(&replaced, "clause 5.6.19 replaces although clause 5.6.16 already removed the history");

    let body = stored_entity(client, &id).await;
    assert_eq!(body["temperature"]["value"], json!(5.5), "clause 5.6.19 replaced the Attribute: {body}");

    // The evolution the entity write recreated must still be the Entity's, not
    // one named after the attribute family of the fragment that was written.
    let appended = client
        .append_entity_attrs(&id, &json!({"humidity": {"type": "Property", "value": 44}, "@context": [TEST_SUITE_CONTEXT]}))
        .await
        .expect("append attributes request");
    assert_write_succeeded(&appended, "clause 5.6.3 appends after a replaced attribute");
    let evolution = stored_temporal_evolution(client, &id).await;
    assert_eq!(
        evolution["type"],
        json!("Building"),
        "clause 4.5.6 records the Temporal Evolution under the Entity's own clause 4.5.1 types: {evolution}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The control for the recreation. Clause 5.6.5 Delete Attribute records the
/// clause 4.5.7 ending of an instance, and clauses 4.5.7 and 4.5.8 make that
/// ending "an instance of the Property [...] recorded with its value set to the
/// URI urn:ngsi-ld:null and the deletedAt Temporal Property set" — one more
/// instance of a sequence. An evolution clause 5.6.16 removed holds no sequence,
/// so the deletion has nothing to end and must not bring the evolution back:
/// doing so would assert instances the client's clause 5.6.16 request removed.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_an_attribute_delete_does_not_recreate_the_history() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:attribute-delete-recreates-nothing");

    create_building(client, &id).await;
    delete_temporal_evolution(client, &id).await;

    let deleted = client
        .delete_entity_attr(&id, "temperature", None, DatasetSelection::Default, Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("delete attribute request");
    assert_write_succeeded(&deleted, "clause 5.6.5 removes the attribute after the history was deleted");

    let retrieved = client
        .get_temporal_entity(&id, "", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(
        retrieved.status,
        StatusCode::NOT_FOUND,
        "a deletion has no instance to end and brings back no Temporal Evolution: {:?}",
        retrieved.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// The not-found control for the write paths. Clauses 5.6.2.4, 5.6.3.4 and
/// 5.6.17.4 each raise `ResourceNotFound` when "there is no existing Entity
/// whose id (URI) [...] is equivalent held locally", and a write that can
/// create a Temporal Evolution must not create an Entity along with it.
#[tokio::test]
async fn regression_temporal_evolution_delete_order_a_write_to_an_unknown_entity_is_still_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:write-to-never-created");
    let fragment = json!({"humidity": {"type": "Property", "value": 44}, "@context": [TEST_SUITE_CONTEXT]});

    let appended = client.append_entity_attrs(&id, &fragment).await.expect("append attributes request");
    assert_resource_not_found(&appended, "clause 5.6.3.4 raises ResourceNotFound for an entity it does not know");

    let updated = client.update_entity_attrs(&id, &fragment).await.expect("update attributes request");
    assert_resource_not_found(&updated, "clause 5.6.2.4 raises ResourceNotFound for an entity it does not know");

    let merged = client.merge_entity(&id, &fragment, None).await.expect("merge entity request");
    assert_resource_not_found(&merged, "clause 5.6.17.4 raises ResourceNotFound for an entity it does not know");

    let retrieved = client
        .get_temporal_entity(&id, "", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(
        retrieved.status,
        StatusCode::NOT_FOUND,
        "a refused write records no Temporal Evolution: {:?}",
        retrieved.body
    );

    scope.finish().await.expect("cleanup failed");
}
