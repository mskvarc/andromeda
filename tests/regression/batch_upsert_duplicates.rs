//! Regression tests for clause 5.5.11, "More than one instance of the same
//! Entity in an Entity array".
//!
//! The defect these cover is a batch coordinator that deduplicated a repeated
//! identifier instead of processing every instance: upsert kept only the last
//! occurrence, and update and merge computed every write for a repeated
//! identifier from one pre-batch snapshot. Both lose what clause 5.5.11
//! requires — the fold across all instances in array order, one temporal
//! observation per instance, and one notification per instance.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The broker path a batch upsert in the optional update mode is submitted to
/// (clause 6.15.3.1: `options=update` selects updating over the default
/// replace).
const UPSERT_UPDATE_PATH: &str = "/ngsi-ld/v1/entityOperations/upsert?options=update";

/// A `Building` carrying exactly one string-valued property.
fn building_with_property(id: &str, attribute: &str, value: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        attribute: {"type": "Property", "value": value},
        "@context": [CORE_CONTEXT]
    })
}

/// A `Building` carrying one numeric property observed at a stated instant.
fn building_observed_at(id: &str, value: i64, observed_at: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "airQualityLevel": {"type": "Property", "value": value, "observedAt": observed_at},
        "@context": [CORE_CONTEXT]
    })
}

/// The instances a temporal representation holds for one attribute.
///
/// An attribute with a single instance may be rendered as one object rather
/// than a one-element array, so both shapes are read and the caller asserts on
/// the count.
fn temporal_instances<'body>(body: &'body Value, attribute: &str) -> Vec<&'body Value> {
    let member = &body[attribute];
    match member {
        Value::Array(instances) => instances.iter().collect(),
        Value::Null => Vec::new(),
        Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => vec![member],
    }
}

/// NGSI-LD 5.5.11.2: in the optional update mode of Batch Upsert, "only the
/// entity resulting from merging all of the entity instances, in the correct
/// order, is maintained in the current state". Two instances of one identifier
/// carrying different attributes must therefore leave the union of both behind:
/// dropping the earlier instance loses the attributes only it named.
#[tokio::test]
async fn regression_clause_5_5_11_2_upsert_update_mode_keeps_the_union_of_every_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:5-5-11-2-update");

    let body = json!([
        building_with_property(&id, "name", "Eiffel Tower"),
        building_with_property(&id, "subCategory", "tourism"),
    ]);
    let response = client
        .raw(Method::POST, UPSERT_UPDATE_PATH)
        .header("Content-Type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("batch upsert failed");
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the first instance creates the entity: {:?}",
        response.body
    );

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let stored = retrieved.json_value().expect("retrieved entity must be valid JSON");
    assert_eq!(
        stored["name"]["value"], "Eiffel Tower",
        "the first instance's attribute must survive the second: {stored}"
    );
    assert_eq!(
        stored["subCategory"]["value"], "tourism",
        "the second instance must be applied on top of the first: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 5.5.11.2: in the default replace mode of Batch Upsert, the first
/// occurrence creates the entity and every subsequent instance replaces it, and
/// "these replace or update operations shall be done in chronological order".
/// The stored entity is therefore the one the last instance states, and the 201
/// body names the entity once however many instances created or replaced it.
#[tokio::test]
async fn regression_clause_5_5_11_2_upsert_replace_mode_applies_the_instances_in_array_order() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:5-5-11-2-replace");

    let body = json!([
        building_with_property(&id, "name", "Eiffel Tower"),
        building_with_property(&id, "subCategory", "tourism"),
    ]);
    let response = client.batch_upsert(&body).await.expect("batch upsert failed");
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the first instance creates the entity: {:?}",
        response.body
    );
    let created = response.json_value().expect("a 201 batch upsert answers with the created identifiers");
    let created = created.as_array().expect("the 201 body is an array of identifiers");
    assert_eq!(created.len(), 1, "an entity is created once however many instances name it: {created:?}");

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let stored = retrieved.json_value().expect("retrieved entity must be valid JSON");
    assert_eq!(
        stored["subCategory"]["value"], "tourism",
        "the instance with the higher array index is applied last: {stored}"
    );
    assert!(
        stored.get("name").is_none(),
        "a replace drops every attribute the replacing instance omits: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 5.5.11.2, last sentence: "For Temporal Evolution of Entities, all
/// entity instances shall be taken into account, in the correct order." A batch
/// naming one entity twice therefore records two observations of the repeated
/// attribute, distinguished by their `observedAt` as clause 5.5.11.0 advises,
/// rather than only the instance that survived a deduplication pass.
#[tokio::test]
async fn regression_clause_5_5_11_2_every_instance_reaches_the_temporal_evolution() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:5-5-11-2-temporal");

    let body = json!([
        building_observed_at(&id, 1, "2020-08-01T12:03:00Z"),
        building_observed_at(&id, 2, "2020-08-01T12:05:00Z"),
    ]);
    let response = client.batch_upsert(&body).await.expect("batch upsert failed");
    assert_eq!(response.status, StatusCode::CREATED, "batch upsert: {:?}", response.body);

    let temporal = client
        .get_temporal_entity(&id, "", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity failed");
    assert_eq!(temporal.status, StatusCode::OK, "temporal retrieve: {:?}", temporal.body);
    let history = temporal.json_value().expect("temporal entity must be valid JSON");
    let instances = temporal_instances(&history, "airQualityLevel");
    let values: Vec<i64> = instances.iter().filter_map(|instance| instance["value"].as_i64()).collect();
    assert_eq!(
        values.len(),
        2,
        "both instances must reach the temporal evolution, not just the surviving one: {history}"
    );
    assert!(values.contains(&1), "the first instance's observation is missing: {history}");
    assert!(values.contains(&2), "the second instance's observation is missing: {history}");

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 5.5.11.5: for Batch Entity Merge, "if an Entity ID is replicated in
/// the array, these merge operations shall be done in chronological order. Only
/// the entity resulting from merging all of the entity instances, in the correct
/// order, is maintained in the current state." Both fragments therefore end up
/// in the stored entity, the later merged onto the earlier one's result.
#[tokio::test]
async fn regression_clause_5_5_11_5_batch_merge_composes_the_instances_in_order() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:5-5-11-5-merge");

    let seed = building_with_property(&id, "name", "Eiffel Tower");
    assert_eq!(
        client.post_entity(&seed).send().await.expect("create entity failed").status,
        StatusCode::CREATED
    );

    let body = json!([
        building_with_property(&id, "name", "Tour Eiffel"),
        building_with_property(&id, "subCategory", "landmark"),
    ]);
    let response = client.batch_merge(&body).await.expect("batch merge failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "batch merge: {:?}", response.body);

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let stored = retrieved.json_value().expect("retrieved entity must be valid JSON");
    assert_eq!(
        stored["name"]["value"], "Tour Eiffel",
        "the first fragment's merge must survive the second: {stored}"
    );
    assert_eq!(
        stored["subCategory"]["value"], "landmark",
        "the second fragment must merge onto the first fragment's result: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 5.5.11.3 with 5.5.11.0: Batch Entity Update processes the instances
/// of a repeated identifier in array order and treats them "as if they had all
/// arrived in separate requests", so each update applies to what its predecessor
/// produced. Computing both writes from one pre-batch snapshot would silently
/// discard the earlier update.
#[tokio::test]
async fn regression_clause_5_5_11_3_batch_update_applies_each_instance_to_the_previous_result() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:5-5-11-3-update");

    let seed = json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "@context": [CORE_CONTEXT]
    });
    assert_eq!(
        client.post_entity(&seed).send().await.expect("create entity failed").status,
        StatusCode::CREATED
    );

    let body = json!([
        building_with_property(&id, "name", "Tour Eiffel"),
        building_with_property(&id, "subCategory", "landmark"),
    ]);
    let response = client.batch_update(&body).await.expect("batch update failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "batch update: {:?}", response.body);

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let stored = retrieved.json_value().expect("retrieved entity must be valid JSON");
    assert_eq!(
        stored["name"]["value"], "Tour Eiffel",
        "the first instance's update must survive the second: {stored}"
    );
    assert_eq!(
        stored["subCategory"]["value"], "landmark",
        "the second instance's update must be applied on top of the first: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 5.5.11.1: for Batch Entity Creation "the first occurrence of an
/// entity in the input array (the oldest one) is used for the creation of the
/// entity. Any subsequent instance of the same entity is reported as an error
/// (entity already exists) in the response." The fold the other batch operations
/// perform must not reach creation, where a repeat is still a conflict.
#[tokio::test]
async fn regression_clause_5_5_11_1_batch_create_reports_already_exists_for_the_later_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:5-5-11-1-create");

    let body = json!([
        building_with_property(&id, "name", "Eiffel Tower"),
        building_with_property(&id, "name", "Tour Eiffel"),
    ]);
    let response = client.batch_create(&body).await.expect("batch create failed");
    assert_eq!(
        response.status,
        StatusCode::MULTI_STATUS,
        "one instance creates and one conflicts: {:?}",
        response.body
    );

    let result = response.json_value().expect("a 207 batch create answers with a BatchOperationResult");
    let success = result["success"].as_array().expect("the result carries a success array");
    let errors = result["errors"].as_array().expect("the result carries an errors array");
    assert_eq!(success.len(), 1, "only the first occurrence creates the entity: {result}");
    assert_eq!(errors.len(), 1, "every later occurrence is reported as an error: {result}");
    assert_eq!(errors[0]["entityId"], id.as_str());
    assert_eq!(
        errors[0]["error"]["type"], "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists",
        "a repeated identifier conflicts exactly as a second create request would: {result}"
    );

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let stored = retrieved.json_value().expect("retrieved entity must be valid JSON");
    assert_eq!(
        stored["name"]["value"], "Eiffel Tower",
        "the oldest instance is the one that created the entity: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 5.5.11.0: "those instances that have the same id are processed by the
/// Broker in the order they have in the array: the higher the index in the
/// array, the later it will be processed. If the order is altered, the outcome
/// may be altered." Two batches holding the same instances in opposite order
/// must therefore leave different entities behind — proof that array order is
/// honoured rather than an arbitrary survivor being kept.
#[tokio::test]
async fn regression_clause_5_5_11_0_reversing_the_array_reverses_the_outcome() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let forward_id = scope.id("Building:5-5-11-0-forward");
    let reversed_id = scope.id("Building:5-5-11-0-reversed");

    let forward = json!([
        building_with_property(&forward_id, "name", "Eiffel Tower"),
        building_with_property(&forward_id, "name", "Tour Eiffel"),
    ]);
    let reversed = json!([
        building_with_property(&reversed_id, "name", "Tour Eiffel"),
        building_with_property(&reversed_id, "name", "Eiffel Tower"),
    ]);
    for body in [&forward, &reversed] {
        let response = client.batch_upsert(body).await.expect("batch upsert failed");
        assert_eq!(response.status, StatusCode::CREATED, "batch upsert: {:?}", response.body);
    }

    for (id, expected) in [(&forward_id, "Tour Eiffel"), (&reversed_id, "Eiffel Tower")] {
        let retrieved = client
            .get_entity(id, "", "application/ld+json", Some(CORE_CONTEXT))
            .await
            .expect("retrieve entity failed");
        assert_eq!(retrieved.status, StatusCode::OK);
        let stored = retrieved.json_value().expect("retrieved entity must be valid JSON");
        assert_eq!(
            stored["name"]["value"], expected,
            "the instance with the higher array index must be the one applied last: {stored}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}
