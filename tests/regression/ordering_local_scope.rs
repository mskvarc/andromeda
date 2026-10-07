//! What makes an ordered query "limited to the local scope" (ETSI GS CIM 009
//! v1.9.1 clauses 4.23.1, 5.5.13, 5.7.2.4, 5.7.4.4).
//!
//! Clause 5.7.2.4 and clause 5.7.4.4 both raise `BadRequestData` when "the
//! ordering parameter is present and the execution of the operation is not
//! limited to the local scope (see clause 5.5.13)". That is not read as the
//! request having asked for local scope: refusing every `orderBy` that does not
//! carry `local=true`, in a broker with an empty Context Registry too, would make
//! clause 4.23.3's "shall be supported" ordering language unreachable in exactly
//! the deployment clause 4.23.1 names, "a Context Consumer [that] queries for
//! Entities retrieved from a single context broker".
//!
//! It is read here as clause 5.5.13 defines it: executing "only ... based on
//! information available in the Context Source or Context Broker directly
//! targeted by the request", whose stated consequence is that "Context Source
//! Registrations from the Context Registry will not be used". A query no
//! registration matches is in that state whether or not it asked to be, so the
//! ordering is admitted; a query a registration takes part in is refused, which
//! is the case clause 4.23.1 wrote the rule for: "Sort ordering is never
//! applied to distributed operations".
//!
//! Each test allocates its own entity type from the environment's prefix, so a
//! registration another test registers concurrently can never decide these
//! answers.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The whole recorded history, which the temporal binding requires even when
/// the query is about ordering rather than about time.
const WHOLE_WINDOW: &str = "timerel=after&timeAt=1970-01-01T00:00:00Z&timeproperty=createdAt";

/// An entity type no other test can be holding a registration for.
///
/// The environment's prefix ends in a UUID, and a type is a term rather than a
/// URI, so the type is composed from that UUID's hexadecimal digits.
fn scoped_type(scope: &TestEnvironment) -> String {
    let unique: String = scope.id_prefix().chars().filter(char::is_ascii_alphanumeric).collect();
    format!("OrderScope{unique}")
}

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

/// Create three entities of `entity_type` whose `name` ascends with their
/// identifier, and return the identifiers in ascending order.
async fn create_ordered_fixtures(scope: &TestEnvironment, client: &TestClient, entity_type: &str) -> Vec<String> {
    let mut ids = Vec::with_capacity(3);
    for (suffix, name) in [("order-1", "A"), ("order-2", "B"), ("order-3", "C")] {
        let id = scope.id(&format!("Building:{suffix}"));
        create(
            client,
            &json!({
                "id": id,
                "type": entity_type,
                "name": {"type": "Property", "value": name},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        ids.push(id);
    }
    ids
}

async fn query(client: &TestClient, query_str: &str) -> (StatusCode, Value) {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    let body = response.json_value().expect("query response must be valid JSON");
    (response.status, body)
}

async fn temporal_query(client: &TestClient, query_str: &str) -> (StatusCode, Value) {
    let response = client
        .query_temporal_entities(query_str, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_temporal_entities failed");
    let body = response.json_value().expect("temporal query response must be valid JSON");
    (response.status, body)
}

async fn post_query(client: &TestClient, body: &Value) -> (StatusCode, Value) {
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/query")
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("post entity query failed");
    let body = response.json_value().expect("query response must be valid JSON");
    (response.status, body)
}

fn ids_from_response(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be a JSON array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("entity must have an id member").to_owned())
        .collect()
}

/// The RFC 7807 `detail` member of a Problem Details body (clause 6.3.2).
fn detail_of(body: &Value) -> &str {
    body["detail"].as_str().expect("a problem detail body must carry a detail member")
}

fn assert_bad_request_data(status: StatusCode, body: &Value, scenario: &str) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{scenario}: {body}");
    assert_eq!(
        body["type"].as_str().expect("a problem detail body must carry a type member"),
        error::BAD_REQUEST_DATA.as_str(),
        "{scenario}: {body}"
    );
}

/// Register a context source covering `entity_type`, so a query on that type is
/// a distributed operation.
///
/// Both read operations are advertised explicitly. Clause 4.20 leaves a
/// registration without an `operations` member advertising `federationOps`,
/// which Table 5.2.9-1 does not extend to the temporal query, so the default
/// would leave the temporal half of these tests with no participating source.
///
/// The endpoint is never contacted: every query below is either refused before
/// forwarding or matched by no registration at all.
async fn register_source(scope: &TestEnvironment, client: &TestClient, label: &str, entity_type: &str) -> String {
    let id = scope.id(&format!("ContextSourceRegistration:{label}"));
    let response = client
        .create_registration(&json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": entity_type}]}],
            "operations": ["queryEntity", "queryTemporal"],
            "endpoint": "http://127.0.0.1:9/unreachable",
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create registration failed");
    assert_eq!(response.status, StatusCode::CREATED, "create registration: {:?}", response.body);
    id
}

/// Clause 4.23.3: "The NGSI-LD Entity Ordering Language shall be supported by
/// implementations for the order parameter". With no registration to distribute
/// to, the execution is limited to the local scope by clause 5.5.13's own
/// definition, and the ordering applies without the request having to say so.
#[tokio::test]
async fn regression_ordering_local_scope_an_entity_query_is_ordered_without_asking_for_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scoped_type(&scope);
    let ids = create_ordered_fixtures(&scope, client, &entity_type).await;

    let (status, body) = query(client, &format!("type={entity_type}&orderBy=name")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), ids, "an ascending ordering sorts by the named member");

    let (status, body) = query(client, &format!("type={entity_type}&orderBy=name%3Bdesc")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut descending = ids.clone(); // clone: the ascending order is asserted above and reused here.
    descending.reverse();
    assert_eq!(ids_from_response(&body), descending, "a descending ordering reverses it");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.23.1: "the behaviour of this clause mirrors the one in clause
/// 6.4.3.2", so the body binding admits the same ordering on the same terms.
#[tokio::test]
async fn regression_ordering_local_scope_the_post_query_binding_is_ordered_without_asking_for_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scoped_type(&scope);
    let ids = create_ordered_fixtures(&scope, client, &entity_type).await;

    let (status, body) = post_query(
        client,
        &json!({
            "type": "Query",
            "entities": [{"type": entity_type}],
            "ordering": {"orderBy": "name;desc"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut descending = ids; // the ascending order the fixtures were created in
    descending.reverse();
    assert_eq!(ids_from_response(&body), descending, "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4 carries clause 5.7.2.4's bullet word for word, so the
/// temporal binding admits an ordering by `id` on the same terms.
#[tokio::test]
async fn regression_ordering_local_scope_a_temporal_query_is_ordered_by_id_without_asking_for_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scoped_type(&scope);
    let ids = create_ordered_fixtures(&scope, client, &entity_type).await;

    let (status, body) = temporal_query(client, &format!("type={entity_type}&{WHOLE_WINDOW}&orderBy=id")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), ids, "{body}");

    let (status, body) = temporal_query(client, &format!("type={entity_type}&{WHOLE_WINDOW}&orderBy=id%3Bdesc")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut descending = ids;
    descending.reverse();
    assert_eq!(ids_from_response(&body), descending, "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4's other ordering bullet is untouched: "If the ordering
/// parameter is present and refers an entity name other than "id", then an
/// error of type `BadRequestData` shall be raised". That one the parameters
/// settle, so it is raised whatever the scope, and it names the member rather
/// than the scope.
#[tokio::test]
async fn regression_ordering_local_scope_a_temporal_ordering_by_another_member_is_refused_for_its_own_reason() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scoped_type(&scope);
    create_ordered_fixtures(&scope, client, &entity_type).await;

    for scope_parameter in ["", "&local=true"] {
        let (status, body) = temporal_query(client, &format!("type={entity_type}&{WHOLE_WINDOW}{scope_parameter}&orderBy=name")).await;
        assert_bad_request_data(status, &body, "a temporal ordering by a member other than id");
        assert!(
            detail_of(&body).contains("id"),
            "the refusal must name the admitted member, not the scope: {}",
            detail_of(&body)
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.1: "Sort ordering is never applied to distributed operations".
/// A registration covering the queried type makes the operation distributed, and
/// the refusal the two clauses mandate is what the query gets -- unless it asks
/// for the local scope, which clause 5.5.13 says leaves the Context Registry
/// unused.
#[tokio::test]
async fn regression_ordering_local_scope_a_matching_registration_refuses_the_ordering() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scoped_type(&scope);
    let ids = create_ordered_fixtures(&scope, client, &entity_type).await;
    let registration = register_source(&scope, client, "matching", &entity_type).await;

    let (status, body) = query(client, &format!("type={entity_type}&orderBy=name")).await;
    assert_bad_request_data(status, &body, "an ordered query a registration takes part in");

    let (status, body) = temporal_query(client, &format!("type={entity_type}&{WHOLE_WINDOW}&orderBy=id")).await;
    assert_bad_request_data(status, &body, "an ordered temporal query a registration takes part in");

    let (status, body) = query(client, &format!("type={entity_type}&local=true&orderBy=name")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), ids, "the local scope admits the same ordering: {body}");

    client.delete_registration(&registration).await.expect("delete registration failed");
    scope.finish().await.expect("cleanup failed");
}

/// The refusal turns on a registration that takes part in *this* query. One
/// covering another type contributes nothing to it, so clause 5.5.13 leaves the
/// execution local and the ordering stands.
#[tokio::test]
async fn regression_ordering_local_scope_a_registration_for_another_type_leaves_the_ordering_admitted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scoped_type(&scope);
    let ids = create_ordered_fixtures(&scope, client, &entity_type).await;
    let registration = register_source(&scope, client, "other-type", &format!("{entity_type}Elsewhere")).await;

    let (status, body) = query(client, &format!("type={entity_type}&orderBy=name")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), ids, "{body}");

    client.delete_registration(&registration).await.expect("delete registration failed");
    scope.finish().await.expect("cleanup failed");
}
