#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Distributed resolution of Linked Entities (NGSI-LD 4.5.23, 4.5.23.1).
//!
//! A Relationship that annotates its object with an `objectType` declares that
//! object an Internal Linked Entity: an entity of the declared type that exists
//! somewhere in the distributed system. When the local store does not hold it,
//! the join reaches it through the matching Context Source Registration. These
//! cases pin what leaves the broker on that forwarded retrieval, and what a
//! client is told when it cannot be answered.

use andromeda_test_harness::environment::TestEnvironment;
use reqwest::{Method, StatusCode};
use std::time::Duration;

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        building_linking_to,
        building_registration,
        client,
        create_entity_local,
        create_registration,
        delete_entity,
        delete_registration,
        get_entity,
        jsonld_link_value,
        minimal_building,
        warning_header_values,
    },
};

/// How long a forwarded retrieval may take to reach the context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The `id` member of every entity in an array body, in response order.
fn ids_from_array(body: &serde_json::Value) -> Vec<String> {
    body.as_array()
        .expect("a joined response is an array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect()
}

/// The decoded query parameters of a captured forwarded request.
fn captured_query_pairs(query: Option<&str>) -> Vec<(String, String)> {
    query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let decode = |raw: &str| urlencoding::decode(raw).expect("a forwarded parameter is percent-encoded UTF-8").into_owned();
            (decode(name), decode(value))
        })
        .collect()
}

/// The value of one decoded query parameter, if the forwarded retrieval carried it.
fn parameter<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}

/// A locally held Building linking to a Building only the context source holds.
struct LinkedFixture {
    /// Identifier of the locally held root.
    root: String,
    /// Identifier of the Linked Entity, held by the context source alone.
    linked: String,
    /// Identifier of the registration pointing at the context source.
    registration: String,
}

/// Provision the root, the registration, and — unless the caller stubs it
/// otherwise — the context source's answer for the Linked Entity.
async fn provision(scope: &TestEnvironment, mock: &MockServer, label: &str, object_type: Option<&str>) -> LinkedFixture {
    let c = client();
    let root = scope.id(&format!("Building:{label}-A"));
    let linked = scope.id(&format!("Building:{label}-R"));
    let registration = scope.id(&format!("Registration:{label}"));

    let (status, _) = create_entity_local(&c, &building_linking_to(&root, &linked, object_type)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_registration(&c, &building_registration(&registration, &linked, &mock.url(""))).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    LinkedFixture { root, linked, registration }
}

/// Remove everything one case provisioned.
async fn discard(fixture: &LinkedFixture) {
    let c = client();
    delete_entity(&c, &fixture.root).await;
    delete_registration(&c, &fixture.registration).await;
}

/// Clause 4.5.23.1: the annotated object is fetched from the context source
/// registered for it, and clause 4.5.23 fixes what the retrieval carries — the
/// levels that remain beyond the fetched entity, and the entities the requester
/// already holds. Two requested levels leave one beyond `R`, so `R`'s own level
/// is spent locally and only `joinLevel=1` travels.
#[tokio::test]
async fn distop_join_fetches_object_type_annotated_remote_link() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let fixture = provision(&scope, &mock, "join-two", Some("Building")).await;
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/entities/{}", fixture.linked),
        200,
        &minimal_building(&fixture.linked),
    )
    .await;

    let (status, _, body) = get_entity(&c, &fixture.root, "join=flat&joinLevel=2", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&fixture.root), "the root must be present: {ids:?}");
    assert!(ids.contains(&fixture.linked), "the remotely held Linked Entity must be present: {ids:?}");

    let captured = mock.next_request(CAPTURE_TIMEOUT).await;
    pretty_assertions::assert_eq!(captured.method, Method::GET);
    pretty_assertions::assert_eq!(captured.uri.path(), format!("/ngsi-ld/v1/entities/{}", fixture.linked));
    let pairs = captured_query_pairs(captured.uri.query());
    pretty_assertions::assert_eq!(parameter(&pairs, "join"), Some("flat"));
    pretty_assertions::assert_eq!(parameter(&pairs, "joinLevel"), Some("1"));
    let contained_by = parameter(&pairs, "containedBy").expect("the forwarded retrieval names the entities already held");
    assert!(
        contained_by.split(',').any(|held| held == fixture.root),
        "containedBy must name the root: {contained_by}"
    );
    pretty_assertions::assert_eq!(mock.get_total_count(), 1, "exactly one retrieval is forwarded");

    discard(&fixture).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.23 admits only a positive `joinLevel`, so a single requested
/// level leaves nothing beyond the fetched entity and the forwarded retrieval
/// is a plain one: no `join`, no `joinLevel`, and only the ids the requester
/// already holds.
#[tokio::test]
async fn distop_join_single_level_forwards_a_plain_retrieval() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let fixture = provision(&scope, &mock, "join-one", Some("Building")).await;
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/entities/{}", fixture.linked),
        200,
        &minimal_building(&fixture.linked),
    )
    .await;

    let (status, _, body) = get_entity(&c, &fixture.root, "join=flat&joinLevel=1", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&fixture.root), "the root must be present: {ids:?}");
    assert!(ids.contains(&fixture.linked), "the remotely held Linked Entity must be present: {ids:?}");

    let captured = mock.next_request(CAPTURE_TIMEOUT).await;
    let pairs = captured_query_pairs(captured.uri.query());
    pretty_assertions::assert_eq!(parameter(&pairs, "join"), None, "a zero-hop join is the absence of one: {pairs:?}");
    pretty_assertions::assert_eq!(parameter(&pairs, "joinLevel"), None, "a zero-hop join carries no level: {pairs:?}");
    // Clause 6.4.3.2 parameter tables: `containedBy` is "Only applicable if
    // joinLevel is present", and a plain retrieval carries no joinLevel.
    pretty_assertions::assert_eq!(parameter(&pairs, "containedBy"), None, "containedBy needs a joinLevel: {pairs:?}");
    pretty_assertions::assert_eq!(mock.get_total_count(), 1, "exactly one retrieval is forwarded");

    discard(&fixture).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.23.1: without an `objectType` the object is not declared an
/// Internal Linked Entity, so an object the local store does not hold stays a
/// plain URI and nothing is forwarded.
#[tokio::test]
async fn distop_join_skips_unannotated_missing_link() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let fixture = provision(&scope, &mock, "join-bare", None).await;
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/entities/{}", fixture.linked),
        200,
        &minimal_building(&fixture.linked),
    )
    .await;

    let (status, _, body) = get_entity(&c, &fixture.root, "join=flat&joinLevel=2", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&fixture.root), "the root must be present: {ids:?}");
    assert!(!ids.contains(&fixture.linked), "an unannotated missing object must stay a plain URI: {ids:?}");
    pretty_assertions::assert_eq!(mock.get_total_count(), 0, "no retrieval may be forwarded");

    discard(&fixture).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.23 leaves an unresolvable Relationship as a plain URI rather than
/// failing the retrieval, and clause 6.3.17 makes `NGSILD-Warning` the only way
/// the client learns the body is incomplete.
#[tokio::test]
async fn distop_join_remote_failure_degrades_to_warning() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let fixture = provision(&scope, &mock, "join-fail", Some("Building")).await;
    mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{}", fixture.linked), 500).await;

    let (status, headers, body) = get_entity(&c, &fixture.root, "join=flat&joinLevel=1", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&fixture.root), "the root must survive the failed join: {ids:?}");
    assert!(!ids.contains(&fixture.linked), "an unresolved Linked Entity must be absent: {ids:?}");

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "an unresolved Linked Entity must be reported as a warning");
    assert!(
        warnings.iter().any(|warning| warning.contains(&fixture.linked)),
        "the warning must name the unresolved Linked Entity: {warnings:?}"
    );

    discard(&fixture).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.23: `containedBy` names the entities the requester already holds.
/// One of them is never fetched again, locally or from a context source.
#[tokio::test]
async fn distop_join_contained_by_prevents_refetch() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let fixture = provision(&scope, &mock, "join-held", Some("Building")).await;
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/entities/{}", fixture.linked),
        200,
        &minimal_building(&fixture.linked),
    )
    .await;

    let query = format!("join=flat&joinLevel=2&containedBy={}", fixture.linked);
    let (status, _, body) = get_entity(&c, &fixture.root, &query, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&fixture.root), "the root must be present: {ids:?}");
    assert!(!ids.contains(&fixture.linked), "an already-held entity must not be returned: {ids:?}");
    pretty_assertions::assert_eq!(mock.get_total_count(), 0, "an already-held entity is never fetched");

    discard(&fixture).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
