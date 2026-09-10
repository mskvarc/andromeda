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

/// One `Building` carrying nothing but an ordinary Property, used to bring the
/// targets of an update or a merge into existence.
fn plain_building(id: &str) -> Value {
    building_with(id, "temperature", &json!({"type": "Property", "value": 21.5}))
}

/// One `Building` whose Property carries the `ngsildproof` sub-Property clause
/// 4.5.2.2 defines the two seals for. This is the positive control: the same
/// members the Relationship clauses prohibit are the intended shape here.
fn proof_sealed_building(id: &str) -> Value {
    building_with(
        id,
        "temperature",
        &json!({
            "type": "Property",
            "value": 21.5,
            "ngsildproof": {
                "type": "Property",
                "value": {"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022", "proofValue": "z2aB"},
                "entityIdSealed": id,
                "entityTypeSealed": "Building"
            }
        }),
    )
}

/// The four prohibited spellings, each on its own `Building`: both members of
/// the clause 4.5.3.2 and 4.5.3.3 "Prohibited" lists, on a Relationship that
/// declares its `type` (normalized, clause 4.5.3.2) and on one that leaves
/// `object` to imply it (concise, clause 4.5.3.3).
fn sealed_relationship_entities(scope: &TestEnvironment, label: &str) -> Vec<(String, Value)> {
    let mut entities = Vec::new();
    for (member, sealed) in [("entityIdSealed", "urn:ngsi-ld:Building:sealed"), ("entityTypeSealed", "Building")] {
        for (spelling, node) in [
            ("normalized", json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1", member: sealed})),
            ("concise", json!({"object": "urn:ngsi-ld:Site:1", member: sealed})),
        ] {
            let id = scope.id(&format!("Building:{label}-{spelling}-{member}"));
            let entity = building_with(&id, "belongsTo", &node);
            entities.push((id, entity));
        }
    }
    entities
}

/// The batch operations whose input array carries entity representations
/// (clause 5.6.7.3, 5.6.8.3, 5.6.9.3 and 5.6.20.3). Batch Entity Delete
/// (clause 5.6.10.3) takes identifiers alone and is therefore not one of them.
#[derive(Clone, Copy, Debug)]
enum BatchWrite {
    /// Batch Entity Creation (clause 5.6.7).
    Create,
    /// Batch Entity Creation or Update, "upsert" (clause 5.6.8).
    Upsert,
    /// Batch Entity Update (clause 5.6.9).
    Update,
    /// Batch Entity Merge (clause 5.6.20).
    Merge,
}

/// Whether a batch operation acts on entities that must already be there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetExistence {
    /// Clauses 5.6.9.4 and 5.6.20.4 update or merge an existing Entity.
    Required,
    /// Clauses 5.6.7.4 and 5.6.8.4 bring the Entity into being themselves.
    NotRequired,
}

impl BatchWrite {
    /// What the operation expects of its targets before it runs.
    const fn target_existence(self) -> TargetExistence {
        match self {
            BatchWrite::Create | BatchWrite::Upsert => TargetExistence::NotRequired,
            BatchWrite::Update | BatchWrite::Merge => TargetExistence::Required,
        }
    }

    /// Send `body` to this operation's endpoint.
    async fn send(self, client: &TestClient, body: &Value) -> TestResponse {
        let response = match self {
            BatchWrite::Create => client.batch_create(body).await,
            BatchWrite::Upsert => client.batch_upsert(body).await,
            BatchWrite::Update => client.batch_update(body).await,
            BatchWrite::Merge => client.batch_merge(body).await,
        };
        response.expect("batch entity request")
    }
}

/// Assert a clause 5.2.16 batch result refused every identifier in `refused`
/// as `BadRequestData`, and left `admitted` out of its error list.
fn assert_batch_refusals(response: &TestResponse, refused: &[String], admitted: &str, scenario: &str) {
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "{scenario}: {:?}", response.body);
    let result = response.json_value().expect("a batch answers with a clause 5.2.16 result");
    let errors = result["errors"].as_array().expect("the clause 5.2.16 result lists its errors");
    for id in refused {
        let error = errors.iter().find(|error| error["entityId"] == json!(id)).unwrap_or(&Value::Null);
        assert_eq!(error["error"]["type"], json!(BAD_REQUEST_DATA), "{scenario}: {id}: {result}");
    }
    assert!(
        !errors.iter().any(|error| error["entityId"] == json!(admitted)),
        "{scenario}: clause 4.5.2.2 admits the seals of the ngsildproof Property: {result}"
    );
}

/// Send the four prohibited spellings plus the admitted `ngsildproof` control
/// through one batch operation and assert exactly the four are refused.
///
/// One request carries all five so the assertion covers the per-entity
/// reporting clauses 5.6.7.4 to 5.6.9.4 and 5.6.20.4 require as well as the
/// refusal itself: an entity the validation refuses contributes a
/// `BatchEntityError` of its own, leaving the rest of the batch to be applied.
async fn assert_batch_refuses_relationship_seals(scope: &TestEnvironment, operation: BatchWrite, label: &str) {
    let client = scope.client();
    let (refused_ids, mut items): (Vec<String>, Vec<Value>) = sealed_relationship_entities(scope, label).into_iter().unzip();
    let admitted = scope.id(&format!("Building:{label}-ngsildproof"));

    if operation.target_existence() == TargetExistence::Required {
        let mut seeded: Vec<Value> = refused_ids.iter().map(|id| plain_building(id)).collect();
        seeded.push(plain_building(&admitted));
        let created = client.batch_create(&Value::Array(seeded)).await.expect("batch create request");
        assert_eq!(created.status, StatusCode::CREATED, "{label} seeding: {:?}", created.body);
    }

    items.push(proof_sealed_building(&admitted));
    let response = operation.send(client, &Value::Array(items)).await;
    assert_batch_refusals(&response, &refused_ids, &admitted, label);
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

/// Clauses 4.5.3.2 and 4.5.3.3 close the Relationship's "Prohibited" list with
/// the same sentence in both representations: "'entityIdSealed' and
/// 'entityTypeSealed' shall never be present". No `ngsildproof` exception
/// applies, because clause 4.5.2.2 grants that one to a Property.
///
/// Create Entity (clause 5.6.1) was the one entity-shaped write path that
/// never ran the pre-expansion check, so a seal forged on a Relationship was
/// created with a 201 and reified into a Property on the way in. The seals are
/// declared only inside `ngsildproof`'s own scoped `@context` (clause 4.4), so
/// after expansion nothing downstream can still tell one from an ordinary
/// sub-attribute.
#[tokio::test]
async fn regression_attribute_member_a_seal_on_a_relationship_is_refused_on_entity_creation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (id, entity) in sealed_relationship_entities(&scope, "create") {
        assert_create_refused(client, &entity, &format!("clauses 4.5.3.2 and 4.5.3.3 prohibit the seal on {id}")).await;
    }

    let admitted = scope.id("Building:create-ngsildproof");
    let created = client
        .post_entity(&proof_sealed_building(&admitted))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "clause 4.5.2.2 defines the ngsildproof Property as the place these two belong: {:?}",
        created.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.7.4: Batch Entity Creation runs clause 5.6.1 "for each of the
/// NGSI-LD Entities included in the input Array", so the prohibition of
/// clauses 4.5.3.2 and 4.5.3.3 holds there too, reported per entity.
#[tokio::test]
async fn regression_attribute_member_a_seal_on_a_relationship_is_refused_by_batch_creation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_batch_refuses_relationship_seals(&scope, BatchWrite::Create, "batch-create").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8.4: Batch Entity Upsert takes the same Entity Array of clause
/// 5.2.4 and carries the same prohibition.
#[tokio::test]
async fn regression_attribute_member_a_seal_on_a_relationship_is_refused_by_batch_upsert() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_batch_refuses_relationship_seals(&scope, BatchWrite::Upsert, "batch-upsert").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.9.4: Batch Entity Update takes the same Entity Array and carries
/// the same prohibition.
#[tokio::test]
async fn regression_attribute_member_a_seal_on_a_relationship_is_refused_by_batch_update() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_batch_refuses_relationship_seals(&scope, BatchWrite::Update, "batch-update").await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.20.4: Batch Entity Merge runs clause 5.6.17 for each entity of
/// the input array, so a seal forged on a Relationship is refused there as
/// well.
#[tokio::test]
async fn regression_attribute_member_a_seal_on_a_relationship_is_refused_by_batch_merge() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    assert_batch_refuses_relationship_seals(&scope, BatchWrite::Merge, "batch-merge").await;

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

/// Clauses 5.6.11.4 and 5.6.12.4 both open with "Execute the behaviour defined
/// in clause 5.5.4 on JSON-LD validation", and clauses 4.5.3.2 and 4.5.3.3
/// prohibit `entityIdSealed` and `entityTypeSealed` on a Relationship in every
/// representation with no exception at all -- the `ngsildproof` exception of
/// clause 4.5.2.2 is a Property's. The temporal write verbs ran no such
/// validation, so a forged seal could be written through
/// `POST /temporal/entities` and through its `/attrs` sibling.
#[tokio::test]
async fn regression_attribute_member_a_temporal_write_refuses_a_relationship_seal() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:temporal-sealed");

    let sealed_instances = json!([
        {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "observedAt": "2024-01-01T00:00:00Z"},
        {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Site:2",
            "observedAt": "2024-01-02T00:00:00Z",
            "entityIdSealed": "urn:ngsi-ld:Building:sealed"
        }
    ]);
    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "Building",
            "belongsTo": sealed_instances,
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity request");
    assert_bad_request_data(&created, "clause 5.6.11.4 runs the clause 5.5.4 validation");

    let retrieved = client
        .get_temporal_entity(&id, "", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(
        retrieved.status,
        StatusCode::NOT_FOUND,
        "a refused temporal write stores nothing: {:?}",
        retrieved.body
    );

    // The same seal through the add-attributes verb of clause 5.6.12, on an
    // evolution that does exist.
    let target = scope.id("Building:temporal-sealed-target");
    let seeded = client
        .create_temporal_entity(&json!({
            "id": target,
            "type": "Building",
            "temperature": [{"type": "Property", "value": 21.5, "observedAt": "2024-01-01T00:00:00Z"}],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity request");
    assert!(seeded.status.is_success(), "temporal seeding: {:?}", seeded.body);

    let appended = client
        .raw(
            http::Method::POST,
            &format!("/ngsi-ld/v1/temporal/entities/{}/attrs", urlencoding::encode(&target)),
        )
        .header("content-type", "application/ld+json")
        .json(&json!({
            "belongsTo": [{"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "entityTypeSealed": "Building"}],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .send()
        .await
        .expect("temporal add attributes request");
    assert_bad_request_data(&appended, "clause 5.6.12.4 runs the clause 5.5.4 validation");

    scope.finish().await.expect("cleanup failed");
}
