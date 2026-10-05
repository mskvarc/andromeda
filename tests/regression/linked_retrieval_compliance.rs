//! Linked Entity retrieval as one rule, read the same way by every operation
//! that joins.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.23 defines Linked Entity retrieval once,
//! and clause 4.21 defines the projection that narrows it. Clauses 5.7.1.5 and
//! 5.7.2.5 apply a restrictive or exclusionary list "to all Entities in the
//! payload in the case of Linked Entity retrieval", and clause 5.8.6 applies
//! `pick`, `omit` and `join` to "every Entity within the payload body" of a
//! notification. The broker had written the rule several times over and the
//! copies had drifted:
//!
//! - notifications dropped a nested `pick` term at compilation and narrowed no
//!   Linked Entity at all (finding R63), and accepted a malformed term;
//! - a plain `pick`, `omit` or `attrs` narrowed only the roots;
//! - two Relationships reaching one flat Linked Entity merged their lists;
//! - `containedBy` applied without `joinLevel`, which Tables 6.4.3.2-1 and
//!   6.5.3.1-1 call "Only applicable if joinLevel is present";
//! - the temporal operations joined unconditionally, although clauses 5.7.3.4,
//!   5.7.4.4 and 5.14.5.4 refuse a Linked Entity reference there; the join is
//!   an extension now, off by default.
//!
//! Every case reads the same four entities: a Building owned and managed by
//! one Person and located in a City, the Person employed by a Company.
//!
//! Naming convention: `regression_linked_compliance_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::sleep;
use urlencoding::encode;

/// The problem type clause 5.5.4 gives data the API data types refuse.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The problem type clause 5.5.3 gives a request the binding does not define.
const INVALID_REQUEST: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";

/// How long a notification that will arrive is waited for.
const NOTIFICATION_WAIT: Duration = Duration::from_secs(15);

/// How long the compiled-subscription cache is given to observe a create.
const CACHE_SETTLE: Duration = Duration::from_millis(600);

/// The temporal window the refusals are asked over: every Core write is
/// recorded on `modifiedAt`, so the window always holds the fixture.
const WINDOW: &str = "timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The token the temporal join extension is advertised under.
const EXTENSION_TOKEN: &str = "temporal-linked-retrieval";

/// The four identifiers of one run's fixture.
struct Fixture {
    building: String,
    person: String,
    city: String,
    company: String,
}

impl Fixture {
    /// Create the fixture under identifiers unique to `label` and this run.
    async fn create(scope: &TestEnvironment, label: &str) -> Fixture {
        let fixture = Fixture {
            building: scope.id(&format!("Building:{label}")),
            person: scope.id(&format!("Person:{label}")),
            city: scope.id(&format!("City:{label}")),
            company: scope.id(&format!("Company:{label}")),
        };
        let client = scope.client();
        create(
            client,
            &json!({
                "id": fixture.company, "type": "Company",
                "name": {"type": "Property", "value": "Acme"},
                "size": {"type": "Property", "value": 50},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        create(
            client,
            &json!({
                "id": fixture.person, "type": "Person",
                "name": {"type": "Property", "value": "Ada"},
                "email": {"type": "Property", "value": "ada@example.com"},
                "employer": {"type": "Relationship", "object": fixture.company},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        create(
            client,
            &json!({
                "id": fixture.city, "type": "City",
                "name": {"type": "Property", "value": "Paris"},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        create(
            client,
            &json!({
                "id": fixture.building, "type": "Building",
                "name": {"type": "Property", "value": "Depot"},
                "owner": {"type": "Relationship", "object": fixture.person},
                "tenant": {"type": "Relationship", "object": fixture.person},
                "locatedIn": {"type": "Relationship", "object": fixture.city},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        fixture
    }
}

/// Create one entity, asserting the broker accepted it.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send one request against a root-relative path under the suite context.
async fn send(client: &TestClient, method: Method, path: &str, body: Option<&Value>) -> TestResponse {
    let request = client
        .raw(method, path)
        .header("Accept", "application/json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"));
    let request = match body {
        Some(body) => request.header("Content-Type", "application/json").json(body),
        None => request,
    };
    request.send().await.expect("request")
}

/// Read one path, asserting a 200, and answer with the body.
async fn read_ok(client: &TestClient, path: &str) -> Value {
    let response = send(client, Method::GET, path, None).await;
    assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
    response.json_value().expect("a 200 carries a JSON body")
}

/// Assert a refusal carries the given problem type.
fn assert_refused(response: &TestResponse, problem_type: &str, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(problem_type), "{scenario}: {problem}");
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

/// The array member carrying `id`, if any.
fn member_with_id<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    members(body).iter().find(|entity| entity["id"].as_str() == Some(id))
}

/// A path's query string with the reserved `{`, `}` and `,` of a clause 4.21
/// projection percent-encoded.
fn projection(text: &str) -> String {
    encode(text).into_owned()
}

/// Bind an isolated notification receiver.
async fn notification_receiver() -> NotificationServer {
    NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification receiver must start")
}

/// A subscription watching the fixture's Building, notified to `endpoint`,
/// with the given members merged into its `notification`.
fn subscription(id: &str, fixture: &Fixture, endpoint: &str, notification: &Value) -> Value {
    let mut members = json!({"endpoint": {"uri": endpoint, "accept": "application/json"}});
    for (key, value) in notification.as_object().expect("notification members are an object") {
        members[key] = value.clone();
    }
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"id": fixture.building, "type": "Building"}],
        "notification": members,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create a subscription, assert it was accepted, and let the cache observe it.
async fn create_subscription(client: &TestClient, body: &Value) {
    let response = client.create_subscription(body).await.expect("create subscription request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
    sleep(CACHE_SETTLE).await;
}

/// Delete a subscription before the assertions run, so it is gone from the
/// shared broker whether the test passes or fails.
async fn delete_subscription(client: &TestClient, id: &str) {
    let response = client.delete_subscription(id).await.expect("delete subscription request");
    assert!(
        response.status == StatusCode::NO_CONTENT || response.status == StatusCode::NOT_FOUND,
        "deleting {id}: {} {:?}",
        response.status,
        response.body
    );
}

/// Change the Building so its subscriptions are evaluated (clause 5.8.6).
async fn trigger(client: &TestClient, fixture: &Fixture) {
    let fragment = json!({"name": {"type": "Property", "value": "Warehouse"}, "@context": [TEST_SUITE_CONTEXT]});
    let response = client.append_entity_attrs(&fixture.building, &fragment).await.expect("append request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{:?}", response.body);
}

/// The `data` member of the first notification `receiver` is sent.
async fn first_notification_data(receiver: &NotificationServer) -> Value {
    let request = receiver.next_request(NOTIFICATION_WAIT).await.expect("a notification arrives");
    let body: Value = request.json().expect("a notification body is JSON");
    body["data"].clone()
}

/// Subscribe with the given notification members, trigger the Building, and
/// answer with the first notification's `data`.
async fn notified_data(scope: &TestEnvironment, fixture: &Fixture, label: &str, notification: &Value) -> Value {
    let client = scope.client();
    let receiver = notification_receiver().await;
    let id = scope.id(&format!("Subscription:{label}"));
    let endpoint = receiver.endpoint_url().expect("notification endpoint");
    create_subscription(client, &subscription(&id, fixture, &endpoint, notification)).await;
    trigger(client, fixture).await;
    let data = first_notification_data(&receiver).await;
    delete_subscription(client, &id).await;
    receiver.shutdown().await.expect("notification receiver shuts down");
    data
}

/// R1. Clause 5.8.6 applies `pick` to every Entity of the payload, and clause
/// 4.21 gives a nested term to the Linked Entity its Relationship reaches.
#[tokio::test]
async fn regression_linked_compliance_a_change_notification_applies_a_nested_pick() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r1").await;

    let data = notified_data(&scope, &fixture, "r1", &json!({"pick": ["id", "owner{name}"], "join": "flat"})).await;
    assert_eq!(members(&data).len(), 2, "the Building and its owner: {data}");
    assert_eq!(keys(&data[0]), set(&["id", "owner"]), "the root keeps what the list names: {data}");
    assert_eq!(keys(&data[1]), set(&["name"]), "the owner keeps what its group names: {data}");
    scope.finish().await.expect("cleanup failed");
}

/// R2. A plain list narrows the Linked Entities too (clause 5.8.6, "every
/// Entity within the payload body").
#[tokio::test]
async fn regression_linked_compliance_a_change_notification_applies_a_plain_pick_to_linked_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r2").await;

    let data = notified_data(&scope, &fixture, "r2", &json!({"pick": ["id", "name", "owner"], "join": "flat"})).await;
    let person = member_with_id(&data, &fixture.person).expect("the owner is appended");
    assert_eq!(keys(person), set(&["id", "name"]), "the plain list narrows the owner: {data}");
    assert!(
        member_with_id(&data, &fixture.city).is_none(),
        "an unreturned Relationship reaches nothing: {data}"
    );
    scope.finish().await.expect("cleanup failed");
}

/// R3. Inline nests a copy under each Relationship, narrowed by the group
/// that Relationship's Linked Entity Term addresses to it (clauses 4.5.23.2
/// and 4.21); a copy no group addresses is whole, and a copy its group
/// empties is not written, while the Relationship keeps its `object`.
#[tokio::test]
async fn regression_linked_compliance_an_inline_notification_nests_per_relationship() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r3").await;

    let data = notified_data(
        &scope,
        &fixture,
        "r3",
        &json!({"pick": ["owner{name}", "tenant", "locatedIn{size}"], "join": "inline"}),
    )
    .await;
    assert_eq!(members(&data).len(), 1, "inline appends nothing: {data}");
    let building = &data[0];
    assert_eq!(keys(&building["owner"]["entity"]), set(&["name"]), "the owner copy keeps its group: {building}");
    assert_eq!(
        keys(&building["tenant"]["entity"]),
        set(&["id", "type", "name", "email", "employer"]),
        "no group addresses the tenant copy, so it is whole: {building}"
    );
    assert_eq!(building["locatedIn"]["object"], json!(fixture.city), "the City keeps its object: {building}");
    assert!(building["locatedIn"].get("entity").is_none(), "the emptied copy is not written: {building}");
    scope.finish().await.expect("cleanup failed");
}

/// R4. A periodic notification is built by the same rule as a change-driven
/// one (clause 5.8.6 governs both).
#[tokio::test]
async fn regression_linked_compliance_a_periodic_notification_applies_the_projection() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = Fixture::create(&scope, "r4").await;
    let receiver = notification_receiver().await;
    let id = scope.id("Subscription:r4");
    let endpoint = receiver.endpoint_url().expect("notification endpoint");
    let mut body = subscription(&id, &fixture, &endpoint, &json!({"pick": ["id", "owner{name}"], "join": "flat"}));
    body["timeInterval"] = json!(2);
    create_subscription(client, &body).await;

    let data = first_notification_data(&receiver).await;
    delete_subscription(client, &id).await;
    receiver.shutdown().await.expect("notification receiver shuts down");

    assert_eq!(members(&data).len(), 2, "the Building and its owner: {data}");
    assert_eq!(keys(&data[0]), set(&["id", "owner"]), "{data}");
    assert_eq!(keys(&data[1]), set(&["name"]), "{data}");
    scope.finish().await.expect("cleanup failed");
}

/// R5. Clause 5.8.1.4 gives a Subscription no Linked Entity admission rule, so
/// a nested term without a join is accepted and honoured: it keeps `owner` on
/// the notified entity, and its group has no Linked Entity to apply to.
#[tokio::test]
async fn regression_linked_compliance_a_subscription_with_a_nested_pick_and_no_join_is_honoured() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r5").await;

    let data = notified_data(&scope, &fixture, "r5", &json!({"pick": ["id", "owner{name}"]})).await;
    assert_eq!(members(&data).len(), 1, "no join appends nothing: {data}");
    assert_eq!(keys(&data[0]), set(&["id", "owner"]), "the root keeps what the list names: {data}");
    scope.finish().await.expect("cleanup failed");
}

/// R6. A nested `pick` reads back as the clause 4.21 text it was written as
/// (Table 5.2.14.1-1, clause 5.5.7).
#[tokio::test]
async fn regression_linked_compliance_a_subscription_reads_back_its_nested_pick() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = Fixture::create(&scope, "r6").await;
    let id = scope.id("Subscription:r6");
    let body = subscription(
        &id,
        &fixture,
        "http://127.0.0.1:9/notify",
        &json!({"pick": ["id", "owner{name,employer{size}}"], "join": "flat", "joinLevel": 2}),
    );
    create_subscription(client, &body).await;

    let response = send(client, Method::GET, &format!("subscriptions/{id}"), None).await;
    delete_subscription(client, &id).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let stored = response.json_value().expect("a subscription is JSON");
    assert_eq!(stored["notification"]["pick"], json!(["id", "owner{name,employer{size}}"]), "{stored}");
    scope.finish().await.expect("cleanup failed");
}

/// R7. Table 5.2.14.1-1 requires "a valid attribute projection language
/// string as per clause 4.21"; an unbalanced group is not one.
#[tokio::test]
async fn regression_linked_compliance_a_malformed_subscription_pick_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = Fixture::create(&scope, "r7").await;
    let id = scope.id("Subscription:r7");
    let body = subscription(&id, &fixture, "http://127.0.0.1:9/notify", &json!({"pick": ["owner{"]}));

    let response = client.create_subscription(&body).await.expect("create subscription request");
    delete_subscription(client, &id).await;
    assert_refused(&response, BAD_REQUEST_DATA, "an unbalanced group");
    scope.finish().await.expect("cleanup failed");
}

/// R8. With the extension off, the temporal operations take no Linked Entity
/// input (clauses 5.7.3.3, 5.7.4.3), refuse a reference to one (clauses
/// 5.7.3.4, 5.7.4.4), leave `join` undefined on their URI binding, and the
/// extension is not advertised.
#[tokio::test]
async fn regression_linked_compliance_temporal_operations_refuse_linked_entity_retrieval_by_default() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = Fixture::create(&scope, "r8").await;
    let building = &fixture.building;

    let nested = send(
        client,
        Method::GET,
        &format!("temporal/entities/{building}?pick={}&{WINDOW}", projection("owner{name}")),
        None,
    )
    .await;
    assert_refused(&nested, BAD_REQUEST_DATA, "(a) a nested temporal pick");

    let hop = send(
        client,
        Method::GET,
        &format!("temporal/entities?type=Building&q={}&{WINDOW}", projection("owner{name}==\"x\"")),
        None,
    )
    .await;
    assert_refused(&hop, BAD_REQUEST_DATA, "(b) a hopping temporal filter");

    let query = json!({
        "type": "Query",
        "entities": [{"id": building, "type": "Building"}],
        "join": "flat",
        "temporalQ": {"timeproperty": "modifiedAt", "timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}
    });
    let joined = send(client, Method::POST, "temporal/entityOperations/query", Some(&query)).await;
    assert_refused(&joined, BAD_REQUEST_DATA, "(c) a retrieving join in a temporal Query body");

    let parameter = send(client, Method::GET, &format!("temporal/entities/{building}?join=flat&{WINDOW}"), None).await;
    assert_refused(&parameter, INVALID_REQUEST, "(d) `join` on the temporal URI binding");

    let identity = client.source_identity(None).await.expect("source identity request");
    assert_eq!(identity.status, StatusCode::OK, "{:?}", identity.body);
    let advertised = identity.json_value().expect("an identity is JSON").to_string();
    assert!(!advertised.contains(EXTENSION_TOKEN), "(e) the extension is not advertised: {advertised}");
    scope.finish().await.expect("cleanup failed");
}

/// R9. Clause 5.7.1.5: a restrictive list "applies to all Entities in the
/// payload in the case of Linked Entity retrieval", and only returned
/// Relationships bring their targets.
#[tokio::test]
async fn regression_linked_compliance_a_plain_pick_reaches_flat_linked_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r9").await;

    let body = read_ok(
        scope.client(),
        &format!("entities/{}?pick={}&join=flat", fixture.building, projection("id,name,owner")),
    )
    .await;
    let person = member_with_id(&body, &fixture.person).expect("the owner is appended");
    assert_eq!(keys(person), set(&["id", "name"]), "the plain list narrows the owner: {body}");
    assert!(
        member_with_id(&body, &fixture.city).is_none(),
        "an unreturned Relationship reaches nothing: {body}"
    );
    scope.finish().await.expect("cleanup failed");
}

/// R10. The exclusionary list reaches the Linked Entities the same way.
#[tokio::test]
async fn regression_linked_compliance_an_omit_reaches_flat_linked_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r10").await;

    let body = read_ok(scope.client(), &format!("entities/{}?omit=email&join=flat", fixture.building)).await;
    let person = member_with_id(&body, &fixture.person).expect("the owner is appended");
    assert!(person.get("email").is_none(), "the omit reaches the owner: {person}");
    assert!(person.get("name").is_some(), "and leaves what it does not name: {person}");
    scope.finish().await.expect("cleanup failed");
}

/// R11. Flat appends each Linked Entity once, "unless a target URI has been
/// previously encountered" (clause 5.7.1.5): the first encounter decides its
/// list, and lists are not merged.
#[tokio::test]
async fn regression_linked_compliance_flat_narrows_by_the_first_encounter() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r11").await;

    let body = read_ok(
        scope.client(),
        &format!("entities/{}?pick={}&join=flat", fixture.building, projection("id,owner{name},tenant{email}")),
    )
    .await;
    assert_eq!(members(&body).len(), 2, "the Person is appended once: {body}");
    assert_eq!(keys(&body[1]), set(&["name"]), "the first encounter, through `owner`, decides: {body}");
    scope.finish().await.expect("cleanup failed");
}

/// R12. Table 6.5.3.1-1: `containedBy` is "Only applicable if joinLevel is
/// present".
#[tokio::test]
async fn regression_linked_compliance_contained_by_needs_join_level() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r12").await;
    let client = scope.client();

    let without = read_ok(client, &format!("entities/{}?join=flat&containedBy={}", fixture.building, fixture.person)).await;
    assert!(member_with_id(&without, &fixture.person).is_some(), "(a) no joinLevel, no exclusion: {without}");

    let with = read_ok(
        client,
        &format!("entities/{}?join=flat&joinLevel=1&containedBy={}", fixture.building, fixture.person),
    )
    .await;
    assert!(member_with_id(&with, &fixture.person).is_none(), "(b) with joinLevel, excluded: {with}");
    scope.finish().await.expect("cleanup failed");
}

/// R13. A group addresses the one entity its Relationship reaches (clause
/// 4.21): the entity reached from there is narrowed only by a group of its
/// own, and is whole without one.
#[tokio::test]
async fn regression_linked_compliance_a_group_addresses_only_the_entity_its_relationship_reaches() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r13").await;

    let body = read_ok(
        scope.client(),
        &format!(
            "entities/{}?pick={}&join=flat&joinLevel=2",
            fixture.building,
            projection("id,owner{name,employer}")
        ),
    )
    .await;
    assert_eq!(members(&body).len(), 3, "the Building, its owner and the owner's employer: {body}");
    assert_eq!(keys(&body[1]), set(&["name", "employer"]), "{body}");
    assert_eq!(keys(&body[2]), set(&["id", "type", "name", "size"]), "no group addresses the employer: {body}");

    let grouped = read_ok(
        scope.client(),
        &format!(
            "entities/{}?pick={}&join=flat&joinLevel=2",
            fixture.building,
            projection("id,owner{name,employer{size}}")
        ),
    )
    .await;
    assert_eq!(keys(&grouped[2]), set(&["size"]), "the employer's own group narrows it: {grouped}");
    scope.finish().await.expect("cleanup failed");
}

/// R14. Table 6.5.3.1-1 makes `attrs` the `pick` synonym, so it reaches the
/// Linked Entities as `pick` does, keeping their identity as `attrs` always does.
#[tokio::test]
async fn regression_linked_compliance_attrs_reaches_flat_linked_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r14").await;

    let body = read_ok(
        scope.client(),
        &format!("entities/{}?attrs={}&join=flat", fixture.building, projection("name,owner")),
    )
    .await;
    let person = member_with_id(&body, &fixture.person).expect("the owner is appended");
    assert_eq!(keys(person), set(&["id", "type", "name"]), "`attrs` narrows the owner: {body}");
    scope.finish().await.expect("cleanup failed");
}

/// R15. One rule, one answer: a notification and a query over the same data
/// narrow the Linked Entity alike (clauses 5.7.2.5 and 5.8.6).
#[tokio::test]
async fn regression_linked_compliance_every_consumer_narrows_alike() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r15").await;

    let data = notified_data(&scope, &fixture, "r15", &json!({"pick": ["id", "owner{name}"], "join": "flat"})).await;
    let queried = read_ok(
        scope.client(),
        &format!("entities?type=Building&id={}&pick={}&join=flat", fixture.building, projection("id,owner{name}")),
    )
    .await;
    assert_eq!(members(&data).len(), members(&queried).len(), "notified {data}, queried {queried}");
    assert_eq!(keys(&data[1]), keys(&queried[1]), "notified {data}, queried {queried}");
    scope.finish().await.expect("cleanup failed");
}

/// R16. A Linked Entity its list empties is not written: inline writes no
/// `entity` (the Relationship keeps its `object`), flat does not append it.
#[tokio::test]
async fn regression_linked_compliance_an_emptied_linked_entity_is_not_written() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r16").await;
    let client = scope.client();

    let inline = read_ok(
        client,
        &format!("entities/{}?pick={}&join=inline", fixture.building, projection("locatedIn{size},owner{name}")),
    )
    .await;
    assert_eq!(inline["locatedIn"]["object"], json!(fixture.city), "(a) the identifier stays: {inline}");
    assert!(inline["locatedIn"].get("entity").is_none(), "(a) the emptied copy is not nested: {inline}");
    assert_eq!(keys(&inline["owner"]["entity"]), set(&["name"]), "(a) the owner copy keeps its group: {inline}");

    let flat = read_ok(
        client,
        &format!("entities/{}?pick={}&join=flat", fixture.building, projection("id,locatedIn{size}")),
    )
    .await;
    assert_eq!(members(&flat).len(), 1, "(b) the City holds no `size`, so it is not appended: {flat}");
    scope.finish().await.expect("cleanup failed");
}

/// R17. Clause 4.5.23.1 retrieves a Relationship "targeting a locally stored
/// Entity" whether or not it carries an `objectType`.
#[tokio::test]
async fn regression_linked_compliance_a_local_target_joins_without_object_type() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let fixture = Fixture::create(&scope, "r17").await;

    let body = read_ok(scope.client(), &format!("entities/{}?join=flat", fixture.building)).await;
    assert!(member_with_id(&body, &fixture.person).is_some(), "the Person is appended: {body}");
    assert!(member_with_id(&body, &fixture.city).is_some(), "the City is appended: {body}");
    assert!(member_with_id(&body, &fixture.company).is_none(), "one level only: {body}");
    scope.finish().await.expect("cleanup failed");
}
