//! What the NGSI-LD Null deletes, once an operation is allowed to carry one.
//!
//! [`crate::null_sentinel_admission`] covers the other half of clause 5.5.4:
//! which operations may carry `urn:ngsi-ld:null` at all. This module assumes
//! admission and asserts the effect: a partial update answers 204 and removes
//! the member rather than storing the sentinel verbatim, so the entity served
//! back no longer carries `"observedAt": "urn:ngsi-ld:null"`; a merge that
//! removes one sub-attribute leaves no valueless instance behind; a merge that
//! removes only `unitCode` is not refused 400, because the deletion instruction
//! never reaches the unit catalog as if it were a code; and a fragment whose
//! `datasetId` names the instance to act on deletes that instance, so a partial
//! update targeting a named instance does not answer 404.
//!
//! The governing clauses:
//!
//! - Clause 4.5.0 defines the encoding. `urn:ngsi-ld:null` stands in for JSON
//!   `null` "in all places where URI strings are valid JSON values"; where an
//!   array is required it is "an array with one single NGSI-LD Null"; and a
//!   `languageMap` spells it `{"@none": "urn:ngsi-ld:null"}`.
//! - Clause 5.5.8 is the Partial Update Patch: a member of the Fragment
//!   replaces the target member wholesale, a member whose value is an NGSI-LD
//!   Null deletes the target member, and "in the case of deleting a specific
//!   Attribute instance with a datasetId, the handling shall be in accordance
//!   with the description found in clause 5.6.5".
//! - Clause 5.5.12 is the Merge Patch, which differs by merging "up to an
//!   arbitrary depth, e.g. including going into JSON objects representing a
//!   Property value" and by the same deletion rule.
//! - Clauses 4.5.2, 4.5.3, 4.5.18, 4.5.21 and 4.5.22 each state that an
//!   NGSI-LD Null on the right-hand side of the family's value member indicates
//!   "a deletion of the Property" (or Relationship) — of the whole attribute,
//!   not of its value alone. That is what makes a sub-attribute written that
//!   way disappear rather than survive valueless.
//! - Clause 5.6.4.4 and clause 5.6.19.4 both select the instance to act on "as
//!   an instance with the specified datasetId if present", and neither
//!   Table 6.7.3.1-1 nor Table 6.7.3.3-1 defines a `datasetId` URL parameter —
//!   so the `datasetId` those clauses mean is the Fragment's own.
//! - Clause 4.5.5 renders multiple instances "as arrays of Attributes, instead
//!   of a single Attribute element", so an attribute reduced to one instance
//!   renders as an object again, `datasetId` and all.
//!
//! One thing the specification says twice and contradicts once is recorded
//! rather than asserted: clause 5.6.4.1 says Partial Attribute update
//! "supports the deletion of sub-Attributes but not the deletion of the whole
//! Attribute itself", while clause 5.5.8 routes instance deletion to clause
//! 5.6.5, which removes the attribute once its last instance is gone. The
//! broker follows clause 5.5.8, and
//! `regression_null_sentinel_deletion_partial_update_of_the_last_instance_removes_the_attribute`
//! records that reading.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The NGSI-LD Null of clause 4.5.0.
const NULL_URI: &str = "urn:ngsi-ld:null";

/// The problem type clause 5.6.4.4 raises for an instance that is not there.
const RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";

/// The two `datasetId` values the multi-instance fixtures are tagged with
/// (clause 4.5.5).
const DATASET_ONE: &str = "urn:ngsi-ld:Dataset:01";
/// The second of them.
const DATASET_TWO: &str = "urn:ngsi-ld:Dataset:02";

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

/// Assert a write was accepted with the 204 clauses 5.6.2.5, 5.6.4.5 and
/// 5.6.17.5 leave without a body.
fn assert_no_content(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{scenario}: {:?}", response.body);
}

/// Assert a refusal is the `ResourceNotFound` of clause 5.6.4.4.
fn assert_resource_not_found(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND), "{scenario}: {problem}");
}

/// An entity carrying one attribute under `name`.
fn entity_with(id: &str, name: &str, attribute: &Value) -> Value {
    json!({"id": id, "type": "Building", name: attribute, "@context": [CORE_CONTEXT]})
}

/// The `airQualityLevel` Property the sub-attribute cases are written over: a
/// value with a `unitCode`, an `observedAt` and a reified sub-attribute, so one
/// fragment can remove one of them and leave the others to be checked.
fn air_quality_level() -> Value {
    json!({
        "type": "Property",
        "value": 4,
        "unitCode": "C62",
        "observedAt": "2020-09-09T16:40:00.000Z",
        "hasWeatherObserved": {"type": "Relationship", "object": "urn:ngsi-ld:Sensor:1"}
    })
}

/// Assert the surviving `airQualityLevel` kept everything but `removed`.
fn assert_only_member_removed(stored: &Value, removed: &str, scenario: &str) {
    let attribute = &stored["airQualityLevel"];
    assert!(attribute.get(removed).is_none(), "{scenario}: '{removed}' survived: {attribute}");
    assert_eq!(attribute["type"], json!("Property"), "{scenario}: {attribute}");
    assert_eq!(attribute["value"], json!(4), "{scenario}: the value must be untouched: {attribute}");
    for member in ["unitCode", "observedAt", "hasWeatherObserved"] {
        if member != removed {
            assert!(attribute.get(member).is_some(), "{scenario}: '{member}' was collateral damage: {attribute}");
        }
    }
}

/// One attribute family, with the value member its own clause gives it and the
/// NGSI-LD Null that deletes an instance of it.
///
/// The two are held together because they are the round trip clause 4.5.0
/// defines: a family's deletion is spelled in the member that family stores its
/// value in, so a table carrying only one of them could name a spelling no
/// family uses.
struct FamilyFixture {
    /// The `type` member of clause 5.2.4, for diagnostics.
    family: &'static str,
    /// A stored instance of that family.
    instance: Value,
    /// The NGSI-LD Null that deletes such an instance, spelled the way that
    /// family's own clause spells it (clauses 4.5.2, 4.5.3, 4.5.18, 4.5.20,
    /// 4.5.21, 4.5.22 and 4.5.24).
    sentinel: Value,
}

/// Every attribute family clause 5.2.4 defines, each with its stored shape and
/// its deletion sentinel.
fn family_fixtures() -> [FamilyFixture; 8] {
    [
        FamilyFixture {
            family: "Property",
            instance: json!({"type": "Property", "value": 4}),
            sentinel: json!({"type": "Property", "value": NULL_URI}),
        },
        FamilyFixture {
            family: "Relationship",
            instance: json!({"type": "Relationship", "object": "urn:ngsi-ld:Sensor:1"}),
            sentinel: json!({"type": "Relationship", "object": NULL_URI}),
        },
        FamilyFixture {
            family: "GeoProperty",
            instance: json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}}),
            sentinel: json!({"type": "GeoProperty", "value": NULL_URI}),
        },
        FamilyFixture {
            family: "LanguageProperty",
            instance: json!({"type": "LanguageProperty", "languageMap": {"fr": "Avenue", "en": "Avenue"}}),
            sentinel: json!({"type": "LanguageProperty", "languageMap": {"@none": NULL_URI}}),
        },
        FamilyFixture {
            family: "JsonProperty",
            instance: json!({"type": "JsonProperty", "json": {"id": 1}}),
            sentinel: json!({"type": "JsonProperty", "json": NULL_URI}),
        },
        FamilyFixture {
            family: "VocabProperty",
            instance: json!({"type": "VocabProperty", "vocab": "Sensor"}),
            sentinel: json!({"type": "VocabProperty", "vocab": NULL_URI}),
        },
        FamilyFixture {
            family: "ListProperty",
            instance: json!({"type": "ListProperty", "valueList": [1, 2]}),
            sentinel: json!({"type": "ListProperty", "valueList": [NULL_URI]}),
        },
        FamilyFixture {
            family: "ListRelationship",
            instance: json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Sensor:1"]}),
            sentinel: json!({"type": "ListRelationship", "objectList": [NULL_URI]}),
        },
    ]
}

/// The same node tagged with a `datasetId`, so it names one instance of a
/// multi-attribute (clause 4.5.5).
fn in_dataset(node: &Value, dataset_id: &str) -> Value {
    let mut tagged = node.clone(); // clone: one fixture serves several cases, each taking its own copy
    tagged["datasetId"] = json!(dataset_id);
    tagged
}

/// Clause 5.5.8: a sub-attribute set to the NGSI-LD Null in a Partial Attribute
/// update is deleted, and nothing else is: the member is not served back holding
/// `urn:ngsi-ld:null`.
#[tokio::test]
async fn regression_null_sentinel_deletion_partial_update_removes_one_sub_attribute_at_a_time() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (member, fragment) in [
        (
            "observedAt",
            json!({"type": "Property", "value": 4, "observedAt": NULL_URI, "@context": [CORE_CONTEXT]}),
        ),
        (
            "unitCode",
            json!({"type": "Property", "value": 4, "unitCode": NULL_URI, "@context": [CORE_CONTEXT]}),
        ),
        (
            "hasWeatherObserved",
            json!({"type": "Property", "value": 4, "hasWeatherObserved": {"type": "Relationship", "object": NULL_URI}, "@context": [CORE_CONTEXT]}),
        ),
    ] {
        let id = scope.id(&format!("Building:partial-update-{member}"));
        create_entity(client, &entity_with(&id, "airQualityLevel", &air_quality_level())).await;

        let patched = client
            .partial_update_attr(&id, "airQualityLevel", &fragment)
            .await
            .expect("partial update attribute request");
        assert_no_content(&patched, member);

        let stored = stored_entity(client, &id).await;
        assert_only_member_removed(&stored, member, member);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12 applies the same deletion rule to Merge Entity. Removing only
/// `unitCode` is not refused, since the deletion instruction is no UN/CEFACT
/// common code to check against the unit catalog; removing a reified
/// sub-attribute removes it whole rather than stripping its `object` and leaving
/// the Relationship behind.
#[tokio::test]
async fn regression_null_sentinel_deletion_merge_removes_one_sub_attribute_at_a_time() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (member, attribute) in [
        ("observedAt", json!({"type": "Property", "value": 4, "observedAt": NULL_URI})),
        ("unitCode", json!({"type": "Property", "value": 4, "unitCode": NULL_URI})),
        (
            "hasWeatherObserved",
            json!({"type": "Property", "value": 4, "hasWeatherObserved": {"type": "Relationship", "object": NULL_URI}}),
        ),
    ] {
        let id = scope.id(&format!("Building:merge-{member}"));
        create_entity(client, &entity_with(&id, "airQualityLevel", &air_quality_level())).await;

        let fragment = entity_with(&id, "airQualityLevel", &attribute);
        let merged = client.merge_entity(&id, &fragment, None).await.expect("merge entity request");
        assert_no_content(&merged, member);

        let stored = stored_entity(client, &id).await;
        assert_only_member_removed(&stored, member, member);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.20.4 runs "the behaviour defined by clause 5.6.17" per entity, so
/// Batch Entity Merge deletes a sub-attribute exactly as Merge Entity does.
#[tokio::test]
async fn regression_null_sentinel_deletion_batch_merge_removes_a_sub_attribute() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:batch-merge-unit-code");

    create_entity(client, &entity_with(&id, "airQualityLevel", &air_quality_level())).await;

    let body = json!([entity_with(
        &id,
        "airQualityLevel",
        &json!({"type": "Property", "value": 4, "unitCode": NULL_URI})
    )]);
    let merged = client.batch_merge(&body).await.expect("batch merge request");
    assert_no_content(&merged, "batch merge deleting a unitCode");

    let stored = stored_entity(client, &id).await;
    assert_only_member_removed(&stored, "unitCode", "batch merge");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.8 selects the instance to act on by the `datasetId` carried in
/// the Fragment, because Table 6.7.3.1-1 gives Partial Attribute update no
/// `datasetId` URL parameter. The `datasetId` is read from the Fragment, not only from a
/// URL parameter, so a Fragment naming an instance deletes it, and a request
/// naming an instance of an attribute that holds no default instance succeeds
/// rather than answering 404.
///
/// Clause 4.5.5 then renders what is left: one surviving instance is "a single
/// Attribute element", not a one-element array.
#[tokio::test]
async fn regression_null_sentinel_deletion_partial_update_deletes_the_instance_the_fragment_names() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for FamilyFixture { family, instance, sentinel } in family_fixtures() {
        let id = scope.id(&format!("Building:instance-{family}"));
        let attribute = json!([in_dataset(&instance, DATASET_ONE), in_dataset(&instance, DATASET_TWO)]);
        create_entity(client, &entity_with(&id, "street", &attribute)).await;

        let mut fragment = in_dataset(&sentinel, DATASET_ONE);
        fragment["@context"] = json!([CORE_CONTEXT]);
        let patched = client
            .partial_update_attr(&id, "street", &fragment)
            .await
            .expect("partial update attribute request");
        assert_no_content(&patched, family);

        let stored = stored_entity(client, &id).await;
        let street = &stored["street"];
        assert!(
            street.is_object(),
            "{family}: one surviving instance renders as an object (clause 4.5.5): {street}"
        );
        assert_eq!(street["datasetId"], json!(DATASET_TWO), "{family}: the wrong instance was deleted: {street}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12 defers instance deletion to the same rule, so a merge Fragment
/// naming a `datasetId` removes that instance and leaves the rest.
#[tokio::test]
async fn regression_null_sentinel_deletion_merge_deletes_the_instance_the_fragment_names() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for FamilyFixture { family, instance, sentinel } in family_fixtures() {
        let id = scope.id(&format!("Building:merge-instance-{family}"));
        let attribute = json!([in_dataset(&instance, DATASET_ONE), in_dataset(&instance, DATASET_TWO)]);
        create_entity(client, &entity_with(&id, "street", &attribute)).await;

        let fragment = entity_with(&id, "street", &in_dataset(&sentinel, DATASET_ONE));
        let merged = client.merge_entity(&id, &fragment, None).await.expect("merge entity request");
        assert_no_content(&merged, family);

        let stored = stored_entity(client, &id).await;
        let street = &stored["street"];
        assert!(
            street.is_object(),
            "{family}: one surviving instance renders as an object (clause 4.5.5): {street}"
        );
        assert_eq!(street["datasetId"], json!(DATASET_TWO), "{family}: the wrong instance was deleted: {street}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.5: "the creation, updating, appending or deleting of Attributes
/// without providing a datasetId only affects the default Attribute instance",
/// and a Fragment that names one only affects that one. Both directions are
/// asserted from the same fixture.
#[tokio::test]
async fn regression_null_sentinel_deletion_selects_the_default_instance_and_the_named_one_separately() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let property = json!({"type": "Property", "value": 4});
    let attribute = json!([property, in_dataset(&json!({"type": "Property", "value": 4}), DATASET_ONE)]);

    let default_deleted = scope.id("Building:delete-default-instance");
    create_entity(client, &entity_with(&default_deleted, "street", &attribute)).await; // clone: both fixtures start from the same attribute
    let fragment = json!({"type": "Property", "value": NULL_URI, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&default_deleted, "street", &fragment)
        .await
        .expect("partial update attribute request");
    assert_no_content(&patched, "deleting the default instance");
    let stored = stored_entity(client, &default_deleted).await;
    assert_eq!(stored["street"]["datasetId"], json!(DATASET_ONE), "the named instance must survive: {stored}");

    let named_deleted = scope.id("Building:delete-named-instance");
    create_entity(client, &entity_with(&named_deleted, "street", &attribute)).await;
    let fragment = json!({"type": "Property", "value": NULL_URI, "datasetId": DATASET_ONE, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&named_deleted, "street", &fragment)
        .await
        .expect("partial update attribute request");
    assert_no_content(&patched, "deleting the named instance");
    let stored = stored_entity(client, &named_deleted).await;
    assert!(
        stored["street"].get("datasetId").is_none(),
        "the default instance must survive, and it never carries a datasetId (clause 4.5.5): {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.8 hands instance deletion to clause 5.6.5, whose description is
/// that "the Attribute itself and all its children shall be deleted" once
/// nothing is left of it. This is the reading that clause 5.6.4.1's sentence
/// about not deleting "the whole Attribute itself" sits against; the module
/// documentation records the tension.
#[tokio::test]
async fn regression_null_sentinel_deletion_partial_update_of_the_last_instance_removes_the_attribute() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:last-instance");

    create_entity(
        client,
        &entity_with(&id, "street", &in_dataset(&json!({"type": "Property", "value": 4}), DATASET_ONE)),
    )
    .await;

    let fragment = json!({"type": "Property", "value": NULL_URI, "datasetId": DATASET_ONE, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&id, "street", &fragment)
        .await
        .expect("partial update attribute request");
    assert_no_content(&patched, "deleting the only instance");

    let stored = stored_entity(client, &id).await;
    assert!(stored.get("street").is_none(), "the attribute must be gone with its last instance: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.4.4: if the target Entity does not contain the target Attribute
/// "as an instance with the specified `datasetId` if present, then an error of
/// type `ResourceNotFound` shall be raised". A deletion Fragment is no exception:
/// a `datasetId` that names nothing is not answered 204.
#[tokio::test]
async fn regression_null_sentinel_deletion_partial_update_of_an_absent_instance_is_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:absent-instance");

    create_entity(
        client,
        &entity_with(&id, "street", &in_dataset(&json!({"type": "Property", "value": 4}), DATASET_ONE)),
    )
    .await;

    let fragment = json!({"type": "Property", "value": NULL_URI, "datasetId": DATASET_TWO, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&id, "street", &fragment)
        .await
        .expect("partial update attribute request");
    assert_resource_not_found(&patched, "a datasetId that names no instance");

    let absent = json!({"type": "Property", "value": NULL_URI, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&id, "colour", &absent)
        .await
        .expect("partial update attribute request");
    assert_resource_not_found(&patched, "an attribute the entity does not carry");

    let stored = stored_entity(client, &id).await;
    assert_eq!(
        stored["street"]["datasetId"],
        json!(DATASET_ONE),
        "neither refusal may change the entity: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.8 EXAMPLE 3 deletes a whole attribute through Update Attributes
/// by setting it to the first-level NGSI-LD Null, and clause 5.5.12 does the
/// same for a merge. Both are asserted here so the first-level spelling is
/// covered beside the value-position one.
#[tokio::test]
async fn regression_null_sentinel_deletion_a_first_level_null_removes_the_whole_attribute() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let updated_id = scope.id("Building:update-attrs-first-level");
    create_entity(client, &entity_with(&updated_id, "airQualityLevel", &air_quality_level())).await;
    let fragment = json!({"airQualityLevel": NULL_URI, "@context": [CORE_CONTEXT]});
    let updated = client.update_entity_attrs(&updated_id, &fragment).await.expect("update attributes request");
    assert_no_content(&updated, "clause 5.6.2 deleting an attribute");
    let stored = stored_entity(client, &updated_id).await;
    assert!(stored.get("airQualityLevel").is_none(), "the attribute must be gone: {stored}");

    let merged_id = scope.id("Building:merge-first-level");
    create_entity(client, &entity_with(&merged_id, "airQualityLevel", &air_quality_level())).await;
    let fragment = json!({"id": merged_id, "type": "Building", "airQualityLevel": NULL_URI, "@context": [CORE_CONTEXT]});
    let merged = client.merge_entity(&merged_id, &fragment, None).await.expect("merge entity request");
    assert_no_content(&merged, "clause 5.6.17 deleting an attribute");
    let stored = stored_entity(client, &merged_id).await;
    assert!(stored.get("airQualityLevel").is_none(), "the attribute must be gone: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.8 replaces a member wholesale where clause 5.5.12 merges into it,
/// so the same Fragment has two different correct outcomes. This is the pair
/// that keeps the deletion rule from being implemented as a merge on both
/// paths.
#[tokio::test]
async fn regression_null_sentinel_deletion_partial_update_replaces_where_merge_descends() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let address = json!({"type": "Property", "value": {"street": "Strasse des 17. Juni", "city": "Berlin", "country": "Germany"}});

    let patched_id = scope.id("Building:value-object-partial-update");
    create_entity(client, &entity_with(&patched_id, "address", &address)).await; // clone: both halves start from the same attribute
    let fragment = json!({"type": "Property", "value": {"street": "Pariser Platz"}, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&patched_id, "address", &fragment)
        .await
        .expect("partial update attribute request");
    assert_no_content(&patched, "clause 5.5.8 replaces the value member");
    let stored = stored_entity(client, &patched_id).await;
    assert_eq!(
        stored["address"]["value"],
        json!({"street": "Pariser Platz"}),
        "clause 5.5.8 replaces rather than merges: {stored}"
    );

    let merged_id = scope.id("Building:value-object-merge");
    create_entity(client, &entity_with(&merged_id, "address", &address)).await;
    let fragment = entity_with(
        &merged_id,
        "address",
        &json!({"type": "Property", "value": {"street": "Pariser Platz", "country": NULL_URI}}),
    );
    let merged = client.merge_entity(&merged_id, &fragment, None).await.expect("merge entity request");
    assert_no_content(&merged, "clause 5.5.12 EXAMPLE 2");
    let stored = stored_entity(client, &merged_id).await;
    assert_eq!(
        stored["address"]["value"],
        json!({"street": "Pariser Platz", "city": "Berlin"}),
        "clause 5.5.12 EXAMPLE 2 updates street and removes country: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 4.5.7 and 4.5.8 render an instance that has ended with the
/// `urn:ngsi-ld:null` its family prescribes, so a deletion writes a tombstone
/// rather than vanishing from the Temporal Evolution. The deletion instruction
/// itself is not an observation: clause 4.5.0 makes it an encoding of `null`,
/// and recording it would report `urn:ngsi-ld:null` as the value of a member
/// the write removed — which for `unitCode` is not even a well-formed one.
#[tokio::test]
async fn regression_null_sentinel_deletion_records_a_tombstone_and_no_sentinel_observation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:temporal-consequence");

    create_entity(client, &entity_with(&id, "airQualityLevel", &air_quality_level())).await;

    let fragment = json!({"type": "Property", "value": 4, "unitCode": NULL_URI, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&id, "airQualityLevel", &fragment)
        .await
        .expect("partial update attribute request");
    assert_no_content(&patched, "removing a unitCode");

    let deletion = json!({"type": "Property", "value": NULL_URI, "@context": [CORE_CONTEXT]});
    let patched = client
        .partial_update_attr(&id, "airQualityLevel", &deletion)
        .await
        .expect("partial update attribute request");
    assert_no_content(&patched, "deleting the attribute");

    let history = client
        .get_temporal_entity(&id, "", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal evolution request");
    assert_eq!(history.status, StatusCode::OK, "temporal retrieve: {:?}", history.body);
    let evolution = history.json_value().expect("a temporal evolution is JSON");
    let instances = evolution["airQualityLevel"]
        .as_array()
        .expect("the temporal evolution of an attribute is an array of instances");

    assert!(
        instances.iter().any(|instance| instance["value"] == json!(NULL_URI)),
        "clause 4.5.7 records the deletion with the family's NGSI-LD Null: {evolution}"
    );
    assert!(
        instances
            .iter()
            .all(|instance| instance.get("unitCode").is_none_or(|unit| unit != &json!(NULL_URI))),
        "a deletion instruction is not an observed unitCode: {evolution}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12 merges "up to an arbitrary depth, e.g. including going into
/// JSON objects representing a Property value", and clause 4.5.24.2 makes a
/// `JsonProperty`'s `json` such a value — it prohibits `value` on the family
/// "as value is a generalization of json". A key inside that raw JSON set to
/// the NGSI-LD Null is therefore removed while the rest of the object stands.
///
/// A genuine JSON `null` in the stored object is data, not a deletion: clause
/// 4.5.0 introduces the URI precisely because "null cannot be used as a value
/// in NGSI-LD", and inside a JSON literal it survives expansion untouched.
#[tokio::test]
async fn regression_null_sentinel_deletion_merge_removes_a_key_inside_a_json_property() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:json-property-key");

    let stored = json!({"type": "JsonProperty", "json": {"id": "some-id", "type": "some-type", "null": null, "anythingElse": "anything-else"}});
    create_entity(client, &entity_with(&id, "jsonProperty", &stored)).await;

    let fragment = entity_with(&id, "jsonProperty", &json!({"type": "JsonProperty", "json": {"anythingElse": NULL_URI}}));
    let merged = client.merge_entity(&id, &fragment, None).await.expect("merge entity request");
    assert_no_content(&merged, "removing one key of a JsonProperty");

    let stored = stored_entity(client, &id).await;
    assert_eq!(
        stored["jsonProperty"]["json"],
        json!({"id": "some-id", "type": "some-type", "null": null}),
        "clause 5.5.12 removes the key and leaves the rest, JSON null included: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.4's third prohibition — `urn:ngsi-ld:null` "as the value of a key
/// value pair within a JSON object, which is the right-hand side of the value
/// of a Property" — is excepted "for NGSI-LD Fragments used in merge operations
/// (see clause 5.5.12)" alone. [`crate::null_sentinel_admission`] asserts that
/// for a Property `value`; this is the `JsonProperty` spelling of the same
/// rule, which reaches the broker as a bare string rather than as an expanded
/// node because clause 4.5.24.2 keeps the value out of JSON-LD interpretation.
#[tokio::test]
async fn regression_null_sentinel_deletion_a_json_property_key_sentinel_is_refused_outside_a_merge() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:json-property-nested-refusal");
    let carrying_sentinel = json!({"type": "JsonProperty", "json": {"anythingElse": NULL_URI}});

    let created = client
        .post_entity(&entity_with(&id, "jsonProperty", &carrying_sentinel))
        .send()
        .await
        .expect("create entity request");
    assert_eq!(
        created.status,
        StatusCode::BAD_REQUEST,
        "clause 5.5.4 excepts merge operations alone: {:?}",
        created.body
    );

    create_entity(
        client,
        &entity_with(&id, "jsonProperty", &json!({"type": "JsonProperty", "json": {"anythingElse": 1}})),
    )
    .await;
    let mut fragment = carrying_sentinel;
    fragment["@context"] = json!([CORE_CONTEXT]);
    let patched = client
        .partial_update_attr(&id, "jsonProperty", &fragment)
        .await
        .expect("partial update attribute request");
    assert_eq!(
        patched.status,
        StatusCode::BAD_REQUEST,
        "clause 5.5.4 does not except a partial update from the nested prohibition: {:?}",
        patched.body
    );

    let stored = stored_entity(client, &id).await;
    assert_eq!(
        stored["jsonProperty"]["json"],
        json!({"anythingElse": 1}),
        "a refused write changes nothing: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}
