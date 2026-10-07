//! Differentiating the conflicting transient versions of an Entity.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.5.2 says that when an Entity is received
//! from a registered Context Source "marked with an expiresAt `DateTime`, but
//! this expiresAt is not duplicated across all versions of the Entity received
//! across all registered Context Sources", then for each version carrying an
//! entity-level `expiresAt`, an Attribute of that version with no `expiresAt`
//! of its own is given one "corresponding to the expiresAt found on the
//! Entity", and an Attribute whose own value "is further in the future than the
//! expiresAt value found on the Entity" is "overwritten to corresponding to the
//! earlier `DateTime`". The merge must not fold the versions and let clause
//! 4.5.5.3 resolve the entity-level value away, which would lose the transience
//! a source reported entirely.
//!
//! Every scenario here registers a Context Source, so every scenario releases
//! its registrations and its mock sources before it asserts anything: a
//! registration left behind by a failed assertion points at a mock port that is
//! no longer listening, and the broker goes on forwarding to it for every later
//! request that matches.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The instant the version that marks the Entity transient expires at.
const ENTITY_EXPIRY: &str = "2099-01-01T00:00:00Z";
/// An Attribute instant further in the future than [`ENTITY_EXPIRY`].
const LATER_THAN_ENTITY: &str = "2100-01-01T00:00:00Z";
/// An Attribute instant earlier than [`ENTITY_EXPIRY`].
const EARLIER_THAN_ENTITY: &str = "2098-01-01T00:00:00Z";

/// What one read produced, kept so every assertion runs after the
/// registrations and the mock sources are gone.
struct Merged {
    /// The status each setup request answered with, in order.
    setup: Vec<StatusCode>,
    /// The status the read itself answered with.
    status: StatusCode,
    /// The Entity the read answered with, or the whole body when the read did
    /// not answer with one.
    entity: Value,
}

impl Merged {
    /// Assert the setup requests were all accepted and the read succeeded,
    /// which is what makes the representation assertions meaningful.
    fn assert_read_succeeded(&self) {
        for status in &self.setup {
            assert!(status.is_success(), "a setup request failed: {status}");
        }
        assert_eq!(self.status, StatusCode::OK, "the read succeeds: {}", self.entity);
    }

    /// The `expiresAt` one member of the merged Entity carries, if any.
    fn member_expiry(&self, member: &str) -> Option<&str> {
        self.entity.get(member).and_then(|member| member.get("expiresAt")).and_then(Value::as_str)
    }

    /// The entity-level `expiresAt`, if any.
    fn entity_expiry(&self) -> Option<&str> {
        self.entity.get("expiresAt").and_then(Value::as_str)
    }
}

/// A registration covering one entity by identifier, under `mode`.
fn registration_for_entity(id: &str, endpoint: &str, mode: &str, entity_type: &str, entity_id: &str, operations: &[&str]) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": mode,
        "operations": operations,
        "information": [{"entities": [{"id": entity_id, "type": entity_type}]}],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A registration covering one Entity type under `mode`, scoped to
/// `entity_id_pattern` so a concurrent test's retrieval of an unrelated
/// identifier does not match it and drag this test's sources into that merge.
fn registration_for_type(id: &str, endpoint: &str, mode: &str, entity_type: &str, entity_id_pattern: &str, operations: &[&str]) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": mode,
        "operations": operations,
        "information": [{"entities": [{"type": entity_type, "idPattern": entity_id_pattern}]}],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Register `body` and report the status without asserting: an assertion here
/// would abandon the registrations already created.
async fn register(client: &TestClient, body: &Value) -> StatusCode {
    client.create_registration(body).await.expect("create registration").status
}

/// Answer the forwarded retrieval of `entity_id` with `body`.
async fn stub_retrieval(source: &ContextSourceAdapter, entity_id: &str, body: &Value) {
    source
        .set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, body)
        .await;
}

/// Answer the forwarded query with the single-entity array `body`.
async fn stub_query(source: &ContextSourceAdapter, body: &Value) {
    source.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &json!([body])).await;
}

/// One version of an Entity as a Context Source would return it.
fn version(entity_id: &str, entity_type: &str, expires_at: Option<&str>, members: &Value) -> Value {
    let mut body = json!({"id": entity_id, "type": entity_type, "@context": TEST_SUITE_CONTEXT});
    let object = body.as_object_mut().expect("the version is a JSON object");
    if let Some(expires_at) = expires_at {
        object.insert("expiresAt".to_owned(), json!(expires_at));
    }
    for (name, member) in members.as_object().into_iter().flatten() {
        object.insert(name.clone(), member.clone()); // clone: the fixture owns the members it composes
    }
    body
}

/// Retrieve the merged Entity.
async fn retrieve(client: &TestClient, entity_id: &str) -> TestResponse {
    client
        .get_entity(entity_id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve entity")
}

/// The body of a read, decoded as an Entity, or the raw text when it is not one.
fn entity_of(response: &TestResponse) -> Value {
    response
        .json_value()
        .unwrap_or_else(|_error| json!({"unparsed": String::from_utf8_lossy(&response.body)}))
}

/// Clause 4.5.5.2, first bullet: the version marked transient has its
/// entity-level instant added to the Attributes of its own that carry none,
/// and the version that reported no `expiresAt` keeps Attributes with none.
#[tokio::test]
async fn regression_transient_conflict_entity_expiry_reaches_the_attributes_that_lack_one() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientPushDownProbe");
    let entity_id = scope.id("Vehicle:transient-push-down");
    let marked = ContextSourceAdapter::start().await;
    let unmarked = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-push-down-marked"),
                &marked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-push-down-unmarked"),
                &unmarked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &marked,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({"speed": {"type": "Property", "value": 60}}),
            ),
        )
        .await;
        stub_retrieval(
            &unmarked,
            &entity_id,
            &version(&entity_id, &entity_type, None, &json!({"fuelLevel": {"type": "Property", "value": 70}})),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![first, second],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    marked.shutdown().await;
    unmarked.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.member_expiry("speed"),
        Some(ENTITY_EXPIRY),
        "the marked version's own Attribute carries its Entity's expiry: {}",
        merged.entity
    );
    assert_eq!(
        merged.member_expiry("fuelLevel"),
        None,
        "the version that reported no expiresAt contributes Attributes with none: {}",
        merged.entity
    );
    assert_eq!(
        merged.entity_expiry(),
        None,
        "clause 4.5.5.3 removes the entity-level expiresAt one version was missing: {}",
        merged.entity
    );
}

/// Clause 4.5.5.2, second bullet: an Attribute whose own `expiresAt` is further
/// in the future than the Entity's is overwritten with the earlier `DateTime`,
/// while one that already expires earlier is left exactly as the source sent it.
#[tokio::test]
async fn regression_transient_conflict_attribute_expiry_is_clamped_only_when_it_is_later() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientClampProbe");
    let entity_id = scope.id("Vehicle:transient-clamp");
    let marked = ContextSourceAdapter::start().await;
    let unmarked = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-clamp-marked"),
                &marked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-clamp-unmarked"),
                &unmarked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &marked,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({
                    "speed": {"type": "Property", "value": 60, "expiresAt": LATER_THAN_ENTITY},
                    "fuelLevel": {"type": "Property", "value": 70, "expiresAt": EARLIER_THAN_ENTITY}
                }),
            ),
        )
        .await;
        stub_retrieval(
            &unmarked,
            &entity_id,
            &version(&entity_id, &entity_type, None, &json!({"tyrePressure": {"type": "Property", "value": 2}})),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![first, second],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    marked.shutdown().await;
    unmarked.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.member_expiry("speed"),
        Some(ENTITY_EXPIRY),
        "an Attribute expiring after its Entity is clamped to the earlier DateTime: {}",
        merged.entity
    );
    assert_eq!(
        merged.member_expiry("fuelLevel"),
        Some(EARLIER_THAN_ENTITY),
        "an Attribute expiring before its Entity keeps its own DateTime: {}",
        merged.entity
    );
}

/// `location`, `observationSpace` and `operationSpace` are Attributes of the
/// Entity (clause 5.2.4), so the clause 4.5.5.2 propagation reaches them the
/// same way it reaches every other Attribute.
///
/// The coordinates are whole numbers deliberately: a forwarded geometry with a
/// fractional coordinate is currently carried verbatim rather than parsed as a
/// `GeoProperty`.
#[tokio::test]
async fn regression_transient_conflict_named_geo_properties_receive_the_propagation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientGeoProbe");
    let entity_id = scope.id("Vehicle:transient-geo");
    let marked = ContextSourceAdapter::start().await;
    let unmarked = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-geo-marked"),
                &marked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-geo-unmarked"),
                &unmarked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &marked,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({
                    "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}},
                    "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [3, 4]}},
                    "operationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [5, 6]}}
                }),
            ),
        )
        .await;
        stub_retrieval(
            &unmarked,
            &entity_id,
            &version(&entity_id, &entity_type, None, &json!({"speed": {"type": "Property", "value": 60}})),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![first, second],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    marked.shutdown().await;
    unmarked.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    for member in ["location", "observationSpace", "operationSpace"] {
        assert_eq!(
            merged.member_expiry(member),
            Some(ENTITY_EXPIRY),
            "{member} is an Attribute of the Entity and takes the propagation: {}",
            merged.entity
        );
    }
}

/// Clause 4.5.2.3: a bare value written where an attribute stands is a concise
/// Property, so the member is an attribute like `speed`. The expiry propagation
/// of clauses 4.5.5.2 and 4.5.5.3 reaches it and its value reads back intact.
#[tokio::test]
async fn regression_transient_conflict_a_concise_member_is_an_attribute_that_takes_the_expiry_and_keeps_its_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientVerbatimProbe");
    let entity_id = scope.id("Vehicle:transient-verbatim");
    let marked = ContextSourceAdapter::start().await;
    let unmarked = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-verbatim-marked"),
                &marked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-verbatim-unmarked"),
                &unmarked.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &marked,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({"speed": {"type": "Property", "value": 60}, "vendorNote": "carried through verbatim"}),
            ),
        )
        .await;
        stub_retrieval(
            &unmarked,
            &entity_id,
            &version(&entity_id, &entity_type, None, &json!({"fuelLevel": {"type": "Property", "value": 70}})),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![first, second],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    marked.shutdown().await;
    unmarked.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.member_expiry("speed"),
        Some(ENTITY_EXPIRY),
        "the attribute beside the verbatim member still takes the propagation: {}",
        merged.entity
    );
    assert_eq!(
        merged.member_expiry("vendorNote"),
        Some(ENTITY_EXPIRY),
        "the concise member is a Property and takes the propagation: {}",
        merged.entity
    );
    assert_eq!(
        merged.entity.get("vendorNote").and_then(|member| member.get("value")).and_then(Value::as_str),
        Some("carried through verbatim"),
        "the value reads back as written: {}",
        merged.entity
    );
}

/// Clause 4.5.5.2 applies only where the `expiresAt` "is not duplicated across
/// all versions": two versions reporting the same instant are not in conflict,
/// so clause 4.5.5.3 keeps the entity-level value and no Attribute gains one.
#[tokio::test]
async fn regression_transient_conflict_versions_that_agree_keep_the_entity_level_expiry() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientAgreedProbe");
    let entity_id = scope.id("Vehicle:transient-agreed");
    let first_source = ContextSourceAdapter::start().await;
    let second_source = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-agreed-1"),
                &first_source.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-agreed-2"),
                &second_source.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &first_source,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({"speed": {"type": "Property", "value": 60}}),
            ),
        )
        .await;
        stub_retrieval(
            &second_source,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({"fuelLevel": {"type": "Property", "value": 70}}),
            ),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![first, second],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    first_source.shutdown().await;
    second_source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.entity_expiry(),
        Some(ENTITY_EXPIRY),
        "clause 4.5.5.3 keeps an expiresAt every version carries: {}",
        merged.entity
    );
    assert_eq!(merged.member_expiry("speed"), None, "no differentiation is called for: {}", merged.entity);
    assert_eq!(merged.member_expiry("fuelLevel"), None, "no differentiation is called for: {}", merged.entity);
}

/// A lone version is duplicated across all the versions there are, so there is
/// no conflict to differentiate and the Entity stays transient as a whole.
#[tokio::test]
async fn regression_transient_conflict_a_single_source_is_not_in_conflict_with_itself() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientLoneProbe");
    let entity_id = scope.id("Vehicle:transient-lone");
    let source = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let registration = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-lone"),
                &source.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &source,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({"speed": {"type": "Property", "value": 60}}),
            ),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![registration],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.entity_expiry(),
        Some(ENTITY_EXPIRY),
        "the only version's expiresAt is the Entity's: {}",
        merged.entity
    );
    assert_eq!(merged.member_expiry("speed"), None, "nothing is differentiated: {}", merged.entity);
}

/// Clauses 5.7.1.4 and 5.7.2.4 make the copy the broker holds itself the base
/// the received versions are merged onto, so it is one of the versions: it
/// triggers the conflict, and its own Attributes are differentiated by it.
#[tokio::test]
async fn regression_transient_conflict_the_locally_held_version_is_a_party() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientLocalPartyProbe");
    let entity_id = scope.id("Vehicle:transient-local-party");
    let source = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let local = json!({
            "id": entity_id,
            "type": entity_type,
            "expiresAt": ENTITY_EXPIRY,
            "speed": {"type": "Property", "value": 60},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let created = client.post_entity(&local).with_ld_json().send().await.expect("create the local version").status;
        let registration = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-local-party"),
                &source.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &source,
            &entity_id,
            &version(&entity_id, &entity_type, None, &json!({"fuelLevel": {"type": "Property", "value": 70}})),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![created, registration],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.entity_expiry(),
        None,
        "a version without an expiresAt removes the entity-level one: {}",
        merged.entity
    );
    assert_eq!(
        merged.member_expiry("speed"),
        Some(ENTITY_EXPIRY),
        "the locally held version keeps its transience on its own Attribute: {}",
        merged.entity
    );
    assert_eq!(
        merged.member_expiry("fuelLevel"),
        None,
        "the source's Attribute is not transient: {}",
        merged.entity
    );
}

/// Clause 4.3.6.2: an auxiliary registration's data "is only included if it is
/// supplementary" and it "never overrides data held directly within a Context
/// Broker". An auxiliary version is therefore not a party to the clause 4.5.5.2
/// conflict, and it cannot erase the entity-level `expiresAt` either.
#[tokio::test]
async fn regression_transient_conflict_an_auxiliary_version_is_not_a_party() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientAuxiliaryProbe");
    let entity_id = scope.id("Vehicle:transient-auxiliary");
    let primary = ContextSourceAdapter::start().await;
    let auxiliary = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-aux-primary"),
                &primary.url("/"),
                "inclusive",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_entity(
                &scope.id("registration:transient-aux-auxiliary"),
                &auxiliary.url("/"),
                "auxiliary",
                &entity_type,
                &entity_id,
                &["retrieveEntity"],
            ),
        )
        .await;
        stub_retrieval(
            &primary,
            &entity_id,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({"speed": {"type": "Property", "value": 60}}),
            ),
        )
        .await;
        stub_retrieval(
            &auxiliary,
            &entity_id,
            &version(&entity_id, &entity_type, None, &json!({"fuelLevel": {"type": "Property", "value": 70}})),
        )
        .await;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![first, second],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    primary.shutdown().await;
    auxiliary.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.entity_expiry(),
        Some(ENTITY_EXPIRY),
        "an auxiliary source reporting no expiresAt does not erase the Entity's: {}",
        merged.entity
    );
    assert_eq!(
        merged.member_expiry("speed"),
        None,
        "an auxiliary version cannot trigger the differentiation: {}",
        merged.entity
    );
    assert!(
        merged.entity.get("fuelLevel").is_some(),
        "the auxiliary source still supplements the missing Attribute: {}",
        merged.entity
    );
}

/// The query path merges the same versions the retrieval does (clause 5.7.2.4),
/// so it differentiates them the same way.
#[tokio::test]
async fn regression_transient_conflict_the_query_path_differentiates_the_same_way() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientQueryProbe");
    let entity_id = scope.id("Vehicle:transient-query");
    let marked = ContextSourceAdapter::start().await;
    let unmarked = ContextSourceAdapter::start().await;

    let merged = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_type(
                &scope.id("registration:transient-query-marked"),
                &marked.url("/"),
                "inclusive",
                &entity_type,
                &format!("^{}:", scope.id_prefix()),
                &["queryEntity"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_type(
                &scope.id("registration:transient-query-unmarked"),
                &unmarked.url("/"),
                "inclusive",
                &entity_type,
                &format!("^{}:", scope.id_prefix()),
                &["queryEntity"],
            ),
        )
        .await;
        stub_query(
            &marked,
            &version(
                &entity_id,
                &entity_type,
                Some(ENTITY_EXPIRY),
                &json!({"speed": {"type": "Property", "value": 60}}),
            ),
        )
        .await;
        stub_query(
            &unmarked,
            &version(&entity_id, &entity_type, None, &json!({"fuelLevel": {"type": "Property", "value": 70}})),
        )
        .await;
        let response = client
            .query_entities(&format!("type={entity_type}"), "application/ld+json", Some(TEST_SUITE_CONTEXT.as_str()))
            .await
            .expect("query entities");
        let entities = entity_of(&response);
        let entity = entities
            .as_array()
            .into_iter()
            .flatten()
            .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(entity_id.as_str()))
            .cloned() // clone: the assertions own the entity while the array is dropped with the response
            .unwrap_or(entities);
        Merged {
            setup: vec![first, second],
            status: response.status,
            entity,
        }
    };
    marked.shutdown().await;
    unmarked.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    merged.assert_read_succeeded();
    assert_eq!(
        merged.member_expiry("speed"),
        Some(ENTITY_EXPIRY),
        "the marked version's Attribute carries its Entity's expiry: {}",
        merged.entity
    );
    assert_eq!(merged.member_expiry("fuelLevel"), None, "the unmarked version's does not: {}", merged.entity);
    assert_eq!(merged.entity_expiry(), None, "the entity-level value is resolved away: {}", merged.entity);
}

/// Clause 4.5.5.2 is scoped to Entities "received from a registered Context
/// Source". A purely local read has no version to conflict with, so an
/// entity-level `expiresAt` never reaches the Attributes of a locally held
/// Entity.
#[tokio::test]
async fn regression_transient_conflict_a_local_read_leaves_attribute_expiry_independent() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TransientLocalOnlyProbe");
    let entity_id = scope.id("Vehicle:transient-local-only");

    let read = {
        let client = scope.client();
        let local = json!({
            "id": entity_id,
            "type": entity_type,
            "expiresAt": ENTITY_EXPIRY,
            "speed": {"type": "Property", "value": 60, "expiresAt": EARLIER_THAN_ENTITY},
            "fuelLevel": {"type": "Property", "value": 70},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let created = client.post_entity(&local).with_ld_json().send().await.expect("create the local entity").status;
        let response = retrieve(client, &entity_id).await;
        Merged {
            setup: vec![created],
            status: response.status,
            entity: entity_of(&response),
        }
    };
    scope.finish().await.expect("cleanup failed");

    read.assert_read_succeeded();
    assert_eq!(read.entity_expiry(), Some(ENTITY_EXPIRY), "the local Entity stays transient: {}", read.entity);
    assert_eq!(
        read.member_expiry("speed"),
        Some(EARLIER_THAN_ENTITY),
        "an Attribute's own expiresAt is independent of the Entity's: {}",
        read.entity
    );
    assert_eq!(
        read.member_expiry("fuelLevel"),
        None,
        "no expiresAt is invented for a locally held Attribute: {}",
        read.entity
    );
}
