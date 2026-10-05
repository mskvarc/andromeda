//! `temporal_linked_retrieval`: clause 4.5.23 Linked Entity retrieval on the
//! Temporal API.
//!
//! Clauses 5.7.3.4, 5.7.4.4 and 5.14.5.4 refuse a Linked Entity reference on
//! the temporal operations, so Andromeda offers the temporal join only as an
//! extension, off by default. Switched on, it applies the Core rules unchanged
//! over the request's window: the same admission row as Retrieve Entity and
//! Query Entities, the same narrowing of every Linked Entity by the list in
//! force where its Relationship was returned, the same first-encounter rule in
//! flat, a narrowed copy under each Relationship in inline, and no Linked
//! Entity written that its list empties.
//!
//! Runs against a broker started with
//! `ANDROMEDA__EXTENSIONS__TEMPORAL_LINKED_RETRIEVAL=enabled` (see
//! `docker/compose/test/extensions/env`).

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use urlencoding::encode;

/// The window every read below is asked over.
const WINDOW: &str = "timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T12:10:00Z";

/// The two instants each fixture attribute was observed at, inside [`WINDOW`].
const INSTANTS: [&str; 2] = ["2020-08-01T12:03:00Z", "2020-08-01T12:05:00Z"];

/// The token the extension is advertised under.
const EXTENSION_TOKEN: &str = "temporal-linked-retrieval";

/// The problem type clause 5.5.4 gives data the API data types refuse.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The four identifiers of one run's fixture: a Building owned and managed by
/// one Person and located in a City, the Person employed by a Company.
struct Fixture {
    building: String,
    person: String,
    company: String,
}

/// Two instances of a Property, one at each of [`INSTANTS`].
fn property(value: &Value) -> Value {
    json!(INSTANTS.map(|instant| json!({"type": "Property", "value": value, "observedAt": instant})))
}

/// Two instances of a Relationship, one at each of [`INSTANTS`].
fn relationship(object: &str) -> Value {
    json!(INSTANTS.map(|instant| json!({"type": "Relationship", "object": object, "observedAt": instant})))
}

impl Fixture {
    /// Create the fixture's history under identifiers unique to `label`.
    async fn create(scope: &TestEnvironment, label: &str) -> Fixture {
        let fixture = Fixture {
            building: scope.id(&format!("Building:{label}")),
            person: scope.id(&format!("Person:{label}")),
            company: scope.id(&format!("Company:{label}")),
        };
        let city = scope.id(&format!("City:{label}"));
        let client = scope.client();
        create(
            client,
            &json!({
                "id": fixture.company, "type": "Company",
                "name": property(&json!("Acme")), "size": property(&json!(50)),
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        create(
            client,
            &json!({
                "id": fixture.person, "type": "Person",
                "name": property(&json!("Ada")), "email": property(&json!("ada@example.com")),
                "employer": relationship(&fixture.company),
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        create(
            client,
            &json!({"id": city, "type": "City", "name": property(&json!("Paris")), "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await;
        create(
            client,
            &json!({
                "id": fixture.building, "type": "Building",
                "name": property(&json!("Depot")),
                "owner": relationship(&fixture.person),
                "tenant": relationship(&fixture.person),
                "locatedIn": relationship(&city),
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        fixture
    }
}

/// Create one Temporal Entity, asserting the broker accepted it.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.create_temporal_entity(entity).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Read one root-relative path under the suite context.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("read request")
}

/// Read one path, asserting a 200, and answer with the body.
async fn read_ok(client: &TestClient, path: &str) -> Value {
    let response = read(client, path).await;
    assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
    response.json_value().expect("a 200 carries a JSON body")
}

/// The member names of one entity.
fn keys(entity: &Value) -> BTreeSet<&str> {
    entity.as_object().expect("an entity is a JSON object").keys().map(String::as_str).collect()
}

/// A set of member names, for comparison with [`keys`].
fn set<'a>(names: &[&'a str]) -> BTreeSet<&'a str> {
    names.iter().copied().collect()
}

/// The members of a collection body, in response order.
fn members(body: &Value) -> &Vec<Value> {
    body.as_array().expect("a joined body is an array")
}

/// The instances one temporal attribute member renders as.
fn instances(member: &Value) -> &Vec<Value> {
    member.as_array().expect("a temporal attribute renders as an array of instances")
}

/// A clause 4.21 projection, percent-encoded for a query string.
fn projection(text: &str) -> String {
    encode(text).into_owned()
}

/// A flat temporal query for the fixture's Building with `parameters`.
async fn flat_query(scope: &TestEnvironment, fixture: &Fixture, parameters: &str) -> Value {
    read_ok(
        scope.client(),
        &format!("temporal/entities?type=Building&id={}&{parameters}&join=flat&{WINDOW}", fixture.building),
    )
    .await
}

/// T1. Every enabled extension is advertised in `contextSourceExtras`.
#[tokio::test]
async fn temporal_linked_retrieval_is_advertised() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let identity = scope.client().source_identity(None).await.expect("source identity request");
    assert_eq!(identity.status, StatusCode::OK, "{:?}", identity.body);
    let advertised = identity.json_value().expect("an identity is JSON").to_string();
    assert!(advertised.contains(EXTENSION_TOKEN), "the extension is advertised: {advertised}");
    scope.finish().await.expect("cleanup");
}

/// T2. A nested group narrows the Linked Entity's history exactly as it
/// narrows a Core Linked Entity, identity included.
#[tokio::test]
async fn temporal_linked_retrieval_a_flat_join_narrows_by_the_core_rules() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let fixture = Fixture::create(&scope, "t2").await;

    let body = flat_query(&scope, &fixture, &format!("pick={}", projection("id,owner{name}"))).await;
    assert_eq!(members(&body).len(), 2, "the Building and its owner: {body}");
    assert_eq!(keys(&body[0]), set(&["id", "owner"]), "{body}");
    assert_eq!(keys(&body[1]), set(&["name"]), "the owner keeps only its group, no identity: {body}");
    scope.finish().await.expect("cleanup");
}

/// T3. Inline nests a copy under every in-window instance of each
/// Relationship, narrowed by the group addressed to it and whole without one;
/// a copy its group empties is not written.
#[tokio::test]
async fn temporal_linked_retrieval_an_inline_join_nests_per_relationship() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let fixture = Fixture::create(&scope, "t3").await;

    let body = read_ok(
        scope.client(),
        &format!(
            "temporal/entities/{}?pick={}&join=inline&{WINDOW}",
            fixture.building,
            projection("owner{name},tenant,locatedIn{size}")
        ),
    )
    .await;
    for instance in instances(&body["owner"]) {
        assert_eq!(keys(&instance["entity"]), set(&["name"]), "each owner copy keeps its group: {body}");
    }
    for instance in instances(&body["tenant"]) {
        assert!(instance["entity"].get("name").is_some(), "no group addresses the tenant copy: {body}");
        assert!(instance["entity"].get("email").is_some(), "no group addresses the tenant copy: {body}");
    }
    for instance in instances(&body["locatedIn"]) {
        assert!(instance["object"].is_string(), "the City keeps its object: {body}");
        assert!(instance.get("entity").is_none(), "the emptied copy is not written: {body}");
    }
    scope.finish().await.expect("cleanup");
}

/// T4. A group addresses the one entity its Relationship reaches, so the
/// entity reached from there is whole.
#[tokio::test]
async fn temporal_linked_retrieval_a_group_addresses_only_the_entity_its_relationship_reaches() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let fixture = Fixture::create(&scope, "t4").await;

    let body = flat_query(&scope, &fixture, &format!("pick={}&joinLevel=2", projection("id,owner{name,employer}"))).await;
    assert_eq!(members(&body).len(), 3, "the Building, its owner and the owner's employer: {body}");
    assert!(body[2].get("name").is_some(), "no group addresses the employer: {body}");
    assert!(body[2].get("size").is_some(), "no group addresses the employer: {body}");
    scope.finish().await.expect("cleanup");
}

/// T5. Only returned Relationships bring their targets, at every level.
#[tokio::test]
async fn temporal_linked_retrieval_flat_drops_what_the_narrowing_left_unnamed() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let fixture = Fixture::create(&scope, "t5").await;

    let body = flat_query(&scope, &fixture, &format!("pick={}&joinLevel=2", projection("id,owner{name}"))).await;
    assert_eq!(
        members(&body).len(),
        2,
        "the owner's `employer` is not returned, so the Company is not reached: {body}"
    );
    assert!(
        members(&body).iter().all(|member| member["id"].as_str() != Some(fixture.company.as_str())),
        "{body}"
    );
    scope.finish().await.expect("cleanup");
}

/// T6. The extension applies the Retrieve Entity admission row: a projection
/// deeper than `joinLevel` is refused (clause 5.7.1.4).
#[tokio::test]
async fn temporal_linked_retrieval_still_bounds_the_depth() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let fixture = Fixture::create(&scope, "t6").await;

    let response = read(
        scope.client(),
        &format!(
            "temporal/entities?type=Building&id={}&pick={}&join=flat&joinLevel=1&{WINDOW}",
            fixture.building,
            projection("owner{employer{name}}")
        ),
    )
    .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{problem}");
    scope.finish().await.expect("cleanup");
}

/// T7. Clause 5.5.14 over the Temporal API: the map a joined temporal query
/// creates records the Linked Entities as well as the roots.
#[tokio::test]
async fn temporal_linked_retrieval_the_entity_map_records_linked_entities() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let fixture = Fixture::create(&scope, "t7").await;
    let client = scope.client();

    let response = read(
        client,
        &format!(
            "temporal/entities?type=Building&id={}&entityMap=true&join=flat&joinLevel=1&{WINDOW}",
            fixture.building
        ),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
    let map_id = response
        .headers
        .get("NGSILD-EntityMap")
        .expect("the map is named")
        .to_str()
        .expect("the map header is ASCII")
        .to_owned();

    let map = client.retrieve_entity_map(&map_id).await.expect("retrieve map request");
    assert_eq!(map.status, StatusCode::OK, "{:?}", map.body);
    let map = map.json_value().expect("a map is JSON");
    let mapped: BTreeSet<&str> = map["entityMap"]
        .as_object()
        .expect("`entityMap` is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert!(mapped.contains(fixture.building.as_str()), "the root is mapped: {map}");
    assert!(mapped.contains(fixture.person.as_str()), "the Linked Entity is mapped: {map}");

    let deleted = client.delete_entity_map(&map_id).await.expect("delete map request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);
    scope.finish().await.expect("cleanup");
}
