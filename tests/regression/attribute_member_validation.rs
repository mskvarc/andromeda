//! Members an attribute family may not carry, and members it must ignore.
//!
//! ETSI GS CIM 009 v1.9.1 closes each family's representation clause with a
//! list of members that "shall never" appear on it, and closes each family's
//! data type definition with the output-only member a Context Producer "shall
//! not" provide. The broker accepted both: a Relationship could be stored with
//! a Property's `value`, and a client could plant a `previousValue` that a
//! later `showChanges` notification would present to subscribers as a state
//! the broker never held.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Assert a refusal is the `BadRequestData` of clauses 5.5.3 and 5.5.4.
fn assert_bad_request_data(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// One `Building` carrying the single attribute under test.
fn building_with(id: &str, attribute: &str, node: &Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        attribute: node,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create `entity` and assert the broker refuses it as `BadRequestData`.
async fn assert_create_refused(client: &TestClient, entity: &Value, scenario: &str) {
    let response = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_bad_request_data(&response, scenario);
}

/// Clause 4.5.3.2: an NGSI-LD Relationship "shall never include" `value`,
/// "as they define a Property value". Storing one would leave the entity
/// naming two families at once, and the rendering of clause 4.5.1 reads only
/// the Relationship's `object`, so the value a client wrote could never be
/// read back.
#[tokio::test]
async fn regression_attribute_member_a_relationship_carrying_a_property_value_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity = building_with(
        &scope.id("Building:relationship-with-value"),
        "belongsTo",
        &json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "value": 42}),
    );
    assert_create_refused(client, &entity, "clause 4.5.3.2 prohibits value on a Relationship").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.2: an NGSI-LD Property "shall never include" `object`, "as
/// they define a Relationship's object URI". The same clause prohibits
/// `objectList`, `languageMap`, `vocab`, `json`, `valueList`, `entity` and
/// `entityList` on a Property for the same reason.
#[tokio::test]
async fn regression_attribute_member_a_property_carrying_a_relationship_object_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity = building_with(
        &scope.id("Building:property-with-object"),
        "temperature",
        &json!({"type": "Property", "value": 21.5, "object": "urn:ngsi-ld:Site:1"}),
    );
    assert_create_refused(client, &entity, "clause 4.5.2.2 prohibits object on a Property").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.18.2: a `LanguageProperty` "shall never include" `value`, "as
/// value is a generalization of languageMap".
#[tokio::test]
async fn regression_attribute_member_a_language_property_carrying_a_value_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity = building_with(
        &scope.id("Building:language-property-with-value"),
        "label",
        &json!({"type": "LanguageProperty", "languageMap": {"en": "Main hall"}, "value": "Main hall"}),
    );
    assert_create_refused(client, &entity, "clause 4.5.18.2 prohibits value on a LanguageProperty").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.21.2: a `ListProperty` "shall never include" `value`, "as value
/// is a generalization of valueList".
#[tokio::test]
async fn regression_attribute_member_a_list_property_carrying_a_value_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity = building_with(
        &scope.id("Building:list-property-with-value"),
        "readings",
        &json!({"type": "ListProperty", "valueList": [1, 2, 3], "value": 1}),
    );
    assert_create_refused(client, &entity, "clause 4.5.21.2 prohibits value on a ListProperty").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.20.2: a `VocabProperty` "shall never include" `value`, "as value
/// is a generalization of vocab".
#[tokio::test]
async fn regression_attribute_member_a_vocab_property_carrying_a_value_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity = building_with(
        &scope.id("Building:vocab-property-with-value"),
        "category",
        &json!({"type": "VocabProperty", "vocab": "office", "value": "office"}),
    );
    assert_create_refused(client, &entity, "clause 4.5.20.2 prohibits value on a VocabProperty").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.2: a `ListRelationship` "shall never include" `object`, "as
/// object is a generalization of objectList".
#[tokio::test]
async fn regression_attribute_member_a_list_relationship_carrying_an_object_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity = building_with(
        &scope.id("Building:list-relationship-with-object"),
        "partOf",
        &json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Site:1"], "object": "urn:ngsi-ld:Site:2"}),
    );
    assert_create_refused(client, &entity, "clause 4.5.22.2 prohibits object on a ListRelationship").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.3.2: `unitCode` "shall never be present" on a Relationship, "as
/// Relationships are unitless". Clauses 4.5.18.2, 4.5.20.2 and 4.5.24.2 say
/// the same of the `LanguageProperty`, `VocabProperty` and `JsonProperty`
/// families.
#[tokio::test]
async fn regression_attribute_member_a_relationship_carrying_a_unit_code_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity = building_with(
        &scope.id("Building:relationship-with-unit-code"),
        "belongsTo",
        &json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "unitCode": "KMH"}),
    );
    assert_create_refused(client, &entity, "clause 4.5.3.2 leaves Relationships unitless").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.2: `entityIdSealed` and `entityTypeSealed` "shall never be
/// present, unless the Property name is `ngsildproof`". The same clause
/// defines `ngsildproof` as the Property that carries them, so the pair is
/// refused anywhere else and accepted there.
///
/// Exercised through the merge of clause 5.6.17, which validates the fragment
/// a client wrote before it is expanded: the seals are declared only inside
/// `ngsildproof`'s own scoped `@context` (clause 4.4), so after expansion a
/// forged one is no longer distinguishable from an ordinary sub-attribute.
#[tokio::test]
async fn regression_attribute_member_a_data_integrity_seal_is_refused_outside_ngsildproof() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:proof-seal");

    let created = client
        .post_entity(&building_with(&id, "temperature", &json!({"type": "Property", "value": 21.5})))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for member in ["entityIdSealed", "entityTypeSealed"] {
        let forged = json!({
            "temperature": {"type": "Property", "value": 22.0, member: "urn:ngsi-ld:Building:other"},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let response = client.merge_entity(&id, &forged, None).await.expect("merge entity request");
        assert_bad_request_data(&response, &format!("clause 4.5.2.2 prohibits {member} outside ngsildproof"));
    }

    let sealed = json!({
        "temperature": {
            "type": "Property",
            "value": 23.0,
            "ngsildproof": {
                "type": "Property",
                "value": {"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022", "proofValue": "z2aB"},
                "entityIdSealed": "urn:ngsi-ld:Building:proof-seal",
                "entityTypeSealed": "Building"
            }
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let accepted = client.merge_entity(&id, &sealed, None).await.expect("merge entity request");
    assert_eq!(
        accepted.status,
        StatusCode::NO_CONTENT,
        "clause 4.5.2.2 defines the ngsildproof Property as the place these two belong: {:?}",
        accepted.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.5: `previousValue` is an output-only member that is "read-only
/// and shall be generated by NGSI-LD implementations; they shall not be
/// provided by Context Producers, and if provided in update or create
/// operations shall be ignored". Ignored, not refused: the create succeeds and
/// the entity is stored without the member, so no later notification can
/// present a client-supplied value as the state that preceded a change.
#[tokio::test]
async fn regression_attribute_member_a_supplied_previous_value_is_ignored_rather_than_stored() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:supplied-previous-value");

    let entity = building_with(&id, "temperature", &json!({"type": "Property", "value": 21.5, "previousValue": 9.9}));
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "clause 5.2.5 ignores the member rather than refusing the request: {:?}",
        created.body
    );

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("the retrieved entity is JSON");
    assert_eq!(body["temperature"]["value"], json!(21.5), "{body}");
    assert!(
        body["temperature"].get("previousValue").is_none(),
        "clause 5.2.5 makes previousValue broker-generated, so a supplied one must not be echoed: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.12: "At least one of (a) entities or (b) watchedAttributes shall
/// be present, unless the member localOnly is set to true, in which case the
/// execution of the request is limited to local scope (see clause 5.5.13)".
#[tokio::test]
async fn regression_attribute_member_a_subscription_naming_neither_entities_nor_watched_attributes_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let subscription = json!({
        "id": scope.id("Subscription:no-entities-no-watched"),
        "type": "Subscription",
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.create_subscription(&subscription).await.expect("create subscription request");
    assert_bad_request_data(&response, "clause 5.2.12 requires entities or watchedAttributes");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.12, Table 5.2.12-1: `entities` is "Mandatory if timeInterval is
/// present, unless the execution of the request is limited to local scope (see
/// clause 5.5.13)". A periodic subscription with nothing to select would fire
/// on a schedule against an unbounded set.
#[tokio::test]
async fn regression_attribute_member_a_periodic_subscription_without_entities_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let subscription = json!({
        "id": scope.id("Subscription:periodic-no-entities"),
        "type": "Subscription",
        "timeInterval": 5,
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.create_subscription(&subscription).await.expect("create subscription request");
    assert_bad_request_data(&response, "clause 5.2.12 makes entities mandatory beside timeInterval");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.14.1, Table 5.2.14.1-1: "showChanges cannot be true in case
/// format is `keyValues`". The simplified rendering of clause 4.5.4 reduces an
/// attribute to its value alone, so it has nowhere to carry the
/// `previousValue` the option exists to deliver; accepting the combination
/// would leave the client holding a subscription whose defining option
/// silently does nothing.
#[tokio::test]
async fn regression_attribute_member_a_subscription_asking_for_show_changes_under_key_values_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let subscription = json!({
        "id": scope.id("Subscription:show-changes-key-values"),
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "keyValues",
            "showChanges": true,
            "endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.create_subscription(&subscription).await.expect("create subscription request");
    assert_bad_request_data(&response, "clause 5.2.14.1 forbids showChanges under keyValues");

    scope.finish().await.expect("cleanup failed");
}
