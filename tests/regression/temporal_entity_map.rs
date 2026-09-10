//! The POST binding of "Query Temporal Evolution of Entities", and the
//! pagination both of its bindings owe their clients.
//!
//! Clause 6.24.3.1 says of `POST /temporal/entityOperations/query` that it "is
//! associated to the operation 'Query Temporal Evolution of Entities' and shall
//! exhibit the behaviour defined by clause 5.7.4", and Table 6.24.3.1-1 answers
//! it with "201 Created (in case an `EntityMap` has been (re)created)" plus "an
//! 'NGSILD-EntityMap' HTTP header that contains the resource URI of the
//! `EntityMap` resource created in the operation". The binding created none,
//! reported none, ignored the `NGSILD-EntityMap` header a follow-up referenced,
//! and dropped the clause 5.2.23 `csf` member on the way to discovery.
//!
//! Clause 4.12 lists "Query Temporal Evolution of Entities (clause 5.7.4)"
//! among the operations that shall "provide a mechanism to flag NGSI-LD Clients
//! when there are remaining NGSI-LD Elements to be traversed as part of a result
//! set" and shall "allow NGSI-LD Clients iterating forwards and backwards
//! through a result set"; clause 6.3.10 binds both to the `Link` relations
//! `next` and `prev`. Neither temporal binding emitted either, so a client had
//! no way to learn that a page was not the whole answer. The two bindings
//! answer it differently, because clause 6.3.10 asks for a Link Target "that
//! could be dereferenced by an NGSI-LD Client to retrieve the next page" and
//! clause 5.5.9.1 for pointers carrying "all the parameters needed [...]
//! without further interactions with the API": the query-string binding emits
//! pointers and the test below follows one, while the POST binding's query is a
//! clause 5.2.23 body no URI-reference carries, so it emits none and flags the
//! remaining elements with the clause 6.3.13 count instead.
//!
//! Clause 6.3.10 also gives the temporal bindings a second, independent
//! pagination dimension — "Temporal representation of resources adds an
//! additional dimension to the pagination" — answered with a 206 and a
//! `Content-Range` over the time range. The two dimensions bound different
//! things, so the last test here holds them on one response at once.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::MockResponse,
    context_source_server::ContextSourceServer,
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, header::CONTENT_TYPE},
};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::iter::once;

/// The clause 6.24.3.1 POST binding of the temporal query.
const TEMPORAL_QUERY: &str = "/ngsi-ld/v1/temporal/entityOperations/query";
/// The clause 6.18.3.2 GET binding of the same operation, which is also the
/// path a forwarded copy of the query reaches a context source on.
const TEMPORAL_ENTITIES: &str = "/ngsi-ld/v1/temporal/entities";
/// The broker's `temporal.default_last_n` safety cap. A history is only cut
/// short — and the response only then a 206 carrying a `Content-Range` — when
/// there are more observations than this and a `lastN` above it.
const SYSTEM_LAST_N_CAP: u32 = 19;

/// A clause 5.2.21 temporal query wide enough to cover every recorded instance.
fn whole_history() -> Value {
    json!({"timerel": "before", "timeAt": "2099-01-01T00:00:00Z"})
}

/// A temporal entity of `entity_type` carrying `count` `speed` observations,
/// one per minute from 2024-01-01T00:00:00Z.
fn temporal_entity(id: &str, entity_type: &str, count: u32) -> Value {
    let instances: Vec<Value> = (0..count)
        .map(|minute| {
            json!({
                "type": "Property",
                "value": f64::from(minute),
                "observedAt": format!("2024-01-01T00:{minute:02}:00Z")
            })
        })
        .collect();
    json!({
        "id": id,
        "type": entity_type,
        "speed": instances,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create a temporal entity and assert the broker accepted it.
async fn create_temporal(client: &TestClient, entity: &Value) {
    let response = client.create_temporal_entity(entity).await.expect("create temporal entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create temporal entity: {:?}", response.body);
}

/// Send a clause 5.2.23 body to the POST binding, optionally under an
/// `EntityMap` and with a clause 6.3.10 page in the query string.
async fn post_query(client: &TestClient, page: &str, body: &Value, map_id: Option<&str>) -> TestResponse {
    let path = if page.is_empty() {
        TEMPORAL_QUERY.to_owned()
    } else {
        format!("{TEMPORAL_QUERY}?{page}")
    };
    let mut request = client
        .raw(Method::POST, &path)
        .header("Accept", "application/ld+json")
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"));
    if let Some(map_id) = map_id {
        request = request.header("NGSILD-EntityMap", map_id);
    }
    request.json(body).send().await.expect("temporal POST query failed")
}

/// Send a query string to the GET binding.
async fn get_query(client: &TestClient, query: &str) -> TestResponse {
    client
        .raw(Method::GET, &format!("{TEMPORAL_ENTITIES}?{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("temporal GET query failed")
}

/// The identifiers a temporal query answered with, in response order.
fn ids_of(response: &TestResponse) -> Vec<String> {
    response
        .json_value()
        .expect("a temporal query body must be JSON")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

/// Every `Link` field value a response carries, joined for inspection.
fn links(response: &TestResponse) -> String {
    response
        .headers
        .get_all("link")
        .into_iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The Link Target clause 6.3.10 gives one relation.
///
/// A `Link` field value is `<target>; rel="…"; type="…"`, and several of them
/// are joined into one field, so the split runs on the angle brackets rather
/// than on a comma an identifier list may itself contain.
fn link_target(response: &TestResponse, relation: &str) -> Option<String> {
    let marker = format!("rel=\"{relation}\"");
    links(response)
        .split('<')
        .skip(1)
        .filter_map(|value| value.split_once('>'))
        .find(|(_, parameters)| parameters.contains(&marker))
        .map(|(target, _)| target.to_owned())
}

/// The `NGSILD-EntityMap` a minting query reported (Table 6.24.3.1-1).
fn reported_map(response: &TestResponse) -> String {
    response
        .headers
        .get("NGSILD-EntityMap")
        .and_then(|value| value.to_str().ok())
        .expect("a minting query answers with the location of its map")
        .to_owned()
}

/// The identifiers an `EntityMap` resource lists (Table 5.2.39-2 `entityMap`).
async fn mapped_ids(client: &TestClient, map_id: &str) -> Vec<String> {
    let response = client.retrieve_entity_map(map_id).await.expect("retrieve EntityMap failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve EntityMap: {:?}", response.body);
    response.json_value().expect("EntityMap body must be JSON")["entityMap"]
        .as_object()
        .expect("an EntityMap renders its entityMap member")
        .keys()
        .cloned()
        .collect()
}

/// The comma-joined, percent-encoded `id` filter that keeps a query string to
/// the entities one isolated test owns.
fn id_filter(ids: &[String]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// Table 6.24.3.1-1: the POST binding answers "201 Created (in case an
/// `EntityMap` has been (re)created)" and "shall include an 'NGSILD-EntityMap'
/// HTTP header that contains the resource URI of the `EntityMap` resource created
/// in the operation". The binding answered 200 with no header at all, so a
/// client asking for a map by the clause 5.2.23 `entityMap` member never learned
/// where its map was — or that one existed.
#[tokio::test]
async fn regression_temporal_entity_map_a_post_query_creates_and_reports_its_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=2).map(|index| scope.id(&format!("Vehicle:tem-mint-{index}"))).collect();
    for id in &ids {
        create_temporal(client, &temporal_entity(id, "Vehicle", 2)).await;
    }

    let response = post_query(
        client,
        "",
        &json!({
            "type": "Query",
            "entities": [{"type": "Vehicle", "id": ids}],
            "temporalQ": whole_history(),
            "entityMap": true
        }),
        None,
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "clause 6.24.3.1: a query that (re)created an EntityMap answers 201: {:?}",
        response.body
    );
    let map_id = reported_map(&response);
    let mut mapped = mapped_ids(client, &map_id).await;
    mapped.sort();
    let mut expected = ids;
    expected.sort();
    assert_eq!(mapped, expected, "the reported map lists exactly the entities the query matched");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14: "If a subsequent request references an existent `EntityMap`, it
/// shall be used for the purposes of Entity registration matching, and queries
/// shall be filtered to only consider the Entities listed [...] An `EntityMap`
/// fixes the Entities to be considered for subsequent requests based on it."
/// The POST binding never read the `NGSILD-EntityMap` request header, so a
/// follow-up under a map was answered as if no map existed.
#[tokio::test]
async fn regression_temporal_entity_map_a_post_follow_up_is_filtered_to_the_mapped_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let mapped: Vec<String> = (1..=2).map(|index| scope.id(&format!("Vehicle:tem-scope-{index}"))).collect();
    let outside = scope.id("Vehicle:tem-scope-outside");
    for id in mapped.iter().chain(once(&outside)) {
        create_temporal(client, &temporal_entity(id, "Vehicle", 2)).await;
    }

    let minted = post_query(
        client,
        "",
        &json!({
            "type": "Query",
            "entities": [{"type": "Vehicle", "id": mapped}],
            "temporalQ": whole_history(),
            "entityMap": true
        }),
        None,
    )
    .await;
    assert_eq!(minted.status, StatusCode::CREATED, "minting query: {:?}", minted.body);
    let map_id = reported_map(&minted);

    // The follow-up names no identifier of its own, so the map alone decides
    // what it considers. The entity outside the map is of the same type and is
    // there to be found, which is what makes the narrowing observable.
    let follow_up = post_query(
        client,
        "limit=100",
        &json!({
            "type": "Query",
            "entities": [{"type": "Vehicle"}],
            "temporalQ": whole_history()
        }),
        Some(&map_id),
    )
    .await;
    assert_eq!(follow_up.status, StatusCode::OK, "map-scoped follow-up: {:?}", follow_up.body);
    let mut answered = ids_of(&follow_up);
    answered.sort();
    let mut expected = mapped;
    expected.sort();
    assert_eq!(
        answered, expected,
        "clause 5.5.14 filters the follow-up to the entities the map lists: {:?}",
        follow_up.body
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14: an `EntityMap` "fixes the Entities to be considered for
/// subsequent requests based on it". A follow-up naming an identifier the map
/// does not list therefore considers no entity at all, and must answer with
/// nothing — never with the entities the map does list, and never with the
/// entity it named.
#[tokio::test]
async fn regression_temporal_entity_map_a_post_follow_up_outside_the_map_returns_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let mapped: Vec<String> = (1..=2).map(|index| scope.id(&format!("Vehicle:tem-outside-{index}"))).collect();
    let outside = scope.id("Vehicle:tem-outside-absent");
    for id in mapped.iter().chain(once(&outside)) {
        create_temporal(client, &temporal_entity(id, "Vehicle", 2)).await;
    }

    let minted = post_query(
        client,
        "",
        &json!({
            "type": "Query",
            "entities": [{"type": "Vehicle", "id": mapped}],
            "temporalQ": whole_history(),
            "entityMap": true
        }),
        None,
    )
    .await;
    assert_eq!(minted.status, StatusCode::CREATED, "minting query: {:?}", minted.body);
    let map_id = reported_map(&minted);

    let unscoped = post_query(
        client,
        "",
        &json!({
            "type": "Query",
            "entities": [{"type": "Vehicle", "id": [outside]}],
            "temporalQ": whole_history()
        }),
        None,
    )
    .await;
    assert_eq!(
        ids_of(&unscoped),
        vec![outside.clone()],
        "without a map the history is there to be found: {:?}",
        unscoped.body
    );

    let scoped = post_query(
        client,
        "",
        &json!({
            "type": "Query",
            "entities": [{"type": "Vehicle", "id": [outside]}],
            "temporalQ": whole_history()
        }),
        Some(&map_id),
    )
    .await;
    assert_eq!(scoped.status, StatusCode::OK, "map-scoped follow-up: {:?}", scoped.body);
    assert_eq!(
        ids_of(&scoped),
        Vec::<String>::new(),
        "clause 5.5.14: an identifier the map does not list leaves nothing to consider: {:?}",
        scoped.body
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.23-1 defines the `csf` member as "a valid query string as per
/// clause 4.9" naming the "Context source filter that shall be matched by
/// Context Source Registrations describing Context Sources to be used for
/// retrieving Entities", and clause 5.7.4.3 lists it among the operation's
/// inputs. The POST binding parsed it out of the body and then passed nothing to
/// discovery, so every registration was contacted whatever the client asked for.
#[tokio::test]
async fn regression_temporal_entity_map_a_post_query_honours_its_context_source_filter() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let configuration = MockServerConfig::from_environment().expect("valid mock server configuration");
    let source = ContextSourceServer::start(configuration).await.expect("context source must start");
    let forwarded_path = CallbackPath::parse(TEMPORAL_ENTITIES).expect("absolute forwarding path");

    let remote = scope.id("CsfTemporalVehicle:remote");
    let mut response_headers = HeaderMap::new();
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
    let remote_history = json!([{
        "id": remote,
        "type": "CsfTemporalVehicle",
        "speed": [{"type": "Property", "value": 42.0, "observedAt": "2024-01-01T00:00:00Z"}],
        "@context": [TEST_SUITE_CONTEXT]
    }]);
    source
        .enqueue(
            Method::GET,
            forwarded_path.clone(), // clone: the mock server owns its route key.
            MockResponse::builder()
                .status(StatusCode::OK)
                .headers(response_headers)
                .body(Bytes::from(serde_json::to_vec(&remote_history).expect("the mock response serializes")))
                .build(),
        )
        .await;

    let endpoint = source
        .endpoint(&CallbackPath::parse("/").expect("absolute root path"))
        .expect("the context source endpoint resolves")
        .to_string();
    let registration_id = scope.id("ContextSourceRegistration:tem-csf");
    let created = client
        .create_registration(&json!({
            "id": registration_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "CsfTemporalVehicle"}]}],
            "endpoint": endpoint,
            "operations": ["queryTemporal"],
            "sourceGroup": {"type": "Property", "value": "alpha"},
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("registration creation failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let declined = post_query(
        client,
        "",
        &json!({
            "type": "Query",
            "entities": [{"type": "CsfTemporalVehicle"}],
            "temporalQ": whole_history(),
            "csf": "sourceGroup==\"beta\""
        }),
        None,
    )
    .await;
    assert_eq!(declined.status, StatusCode::OK, "csf query: {:?}", declined.body);
    assert_eq!(
        ids_of(&declined),
        Vec::<String>::new(),
        "a csf no registration satisfies leaves the source out of the operation: {:?}",
        declined.body
    );
    assert_eq!(
        source.request_count(Method::GET, forwarded_path.clone()), // clone: the count is taken twice against the same route key.
        0,
        "clause 5.7.4.3: the context source filter decides which registrations are contacted"
    );

    let accepted = post_query(
        client,
        "",
        &json!({
            "type": "Query",
            "entities": [{"type": "CsfTemporalVehicle"}],
            "temporalQ": whole_history(),
            "csf": "sourceGroup==\"alpha\""
        }),
        None,
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK, "csf query: {:?}", accepted.body);
    assert_eq!(
        ids_of(&accepted),
        vec![remote],
        "a csf the registration satisfies keeps its source in the operation: {:?}",
        accepted.body
    );
    assert_eq!(
        source.request_count(Method::GET, forwarded_path),
        1,
        "the matching registration is the one that was contacted"
    );

    client.delete_registration(&registration_id).await.expect("delete registration failed");
    source.shutdown().await.expect("context source shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.12 requires "Query Temporal Evolution of Entities (clause 5.7.4)" to
/// "provide a mechanism to flag NGSI-LD Clients when there are remaining NGSI-LD
/// Elements to be traversed as part of a result set" and to "allow NGSI-LD
/// Clients iterating forwards and backwards through a result set"; clause 6.3.10
/// binds those to `Link` relation types `next` and `prev`. The GET binding
/// emitted neither, so a limited page looked exactly like a complete answer.
#[tokio::test]
async fn regression_temporal_pagination_the_get_binding_flags_the_remaining_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=3).map(|index| scope.id(&format!("Vehicle:tem-getpage-{index}"))).collect();
    for id in &ids {
        create_temporal(client, &temporal_entity(id, "Vehicle", 2)).await;
    }
    let selection = format!("type=Vehicle&id={}&timerel=before&timeAt=2099-01-01T00:00:00Z", id_filter(&ids));

    let first = get_query(client, &format!("{selection}&limit=2&offset=0")).await;
    assert_eq!(first.status, StatusCode::OK, "first page: {:?}", first.body);
    assert_eq!(ids_of(&first).len(), 2, "the first page is filled to the limit: {:?}", first.body);
    let first_links = links(&first);
    assert!(
        first_links.contains("rel=\"next\""),
        "clause 6.3.10: a page with elements left to traverse offers a next link, got: {first_links}"
    );
    assert!(
        !first_links.contains("rel=\"prev\""),
        "the first page has nothing before it, got: {first_links}"
    );

    // Clause 6.3.10: "the Link Target shall be a URI-reference that could be
    // dereferenced by an NGSI-LD Client to retrieve the next page of NGSI-LD
    // Elements", and clause 5.5.9.1: the pointers "shall contain all the
    // parameters needed to allow NGSI-LD Clients to retrieve the next and
    // previous page, without further interactions with the API". Following it
    // is the only assertion that says so.
    let next = link_target(&first, "next").expect("clause 6.3.10 gives a page with elements left a next pointer");
    let followed = client
        .raw(Method::GET, &next)
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("the next pointer is dereferenceable");
    assert_eq!(followed.status, StatusCode::OK, "following {next}: {:?}", followed.body);

    let last = get_query(client, &format!("{selection}&limit=2&offset=2")).await;
    assert_eq!(last.status, StatusCode::OK, "last page: {:?}", last.body);
    assert_eq!(ids_of(&last).len(), 1, "the last page carries the remainder: {:?}", last.body);
    assert_eq!(
        ids_of(&followed),
        ids_of(&last),
        "the pointer retrieves the next page, carrying every parameter it needs: {:?}",
        followed.body
    );
    let last_links = links(&last);
    assert!(
        last_links.contains("rel=\"prev\""),
        "clause 6.3.10: a page past the start offers a prev link, got: {last_links}"
    );
    assert!(
        !last_links.contains("rel=\"next\""),
        "clause 4.12: nothing remains to be traversed, so no next link, got: {last_links}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.10 requires of a pagination pointer that "the Link Target shall
/// be a URI-reference that could be dereferenced by an NGSI-LD Client to
/// retrieve the next page of NGSI-LD Elements", and clause 5.5.9.1 that "the
/// pointers to NGSI-LD Elements shall contain all the parameters needed to
/// allow NGSI-LD Clients to retrieve the next and previous page, without
/// further interactions with the API".
///
/// `POST /temporal/entityOperations/query` can satisfy neither. Its query is
/// the clause 5.2.23 body, which no URI-reference carries, and the resource the
/// pointer would name answers only POST, so a client dereferencing it with a
/// GET gets a 405. The binding emits no pointer at all rather than one that
/// misleads, and clause 4.12's "mechanism to flag NGSI-LD Clients when there
/// are remaining NGSI-LD Elements to be traversed" is served here by the clause
/// 6.3.13 `NGSILD-Results-Count` a counted query asks for.
#[tokio::test]
async fn regression_temporal_pagination_the_post_binding_points_at_no_page_it_cannot_serve() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=3).map(|index| scope.id(&format!("Vehicle:tem-postpage-{index}"))).collect();
    for id in &ids {
        create_temporal(client, &temporal_entity(id, "Vehicle", 2)).await;
    }
    let body = json!({
        "type": "Query",
        "entities": [{"type": "Vehicle", "id": ids}],
        "temporalQ": whole_history()
    });

    let first = post_query(client, "limit=2&offset=0&count=true", &body, None).await;
    assert_eq!(first.status, StatusCode::OK, "first page: {:?}", first.body);
    assert_eq!(ids_of(&first).len(), 2, "the first page is filled to the limit: {:?}", first.body);
    let first_links = links(&first);
    assert!(
        !first_links.contains("rel=\"next\""),
        "no URI-reference carries the clause 5.2.23 body, so the binding points at nothing, got: {first_links}"
    );
    assert!(
        !first_links.contains("rel=\"prev\""),
        "the same holds in the other direction, got: {first_links}"
    );
    assert_eq!(
        first.headers.get("NGSILD-Results-Count").and_then(|value| value.to_str().ok()),
        Some("3"),
        "clause 6.3.13: the count is how this binding flags the elements left to traverse"
    );

    // The page itself is still cut where the URL asked, so a client pages by
    // sending the same body again with the next window.
    let last = post_query(client, "limit=2&offset=2", &body, None).await;
    assert_eq!(last.status, StatusCode::OK, "last page: {:?}", last.body);
    assert_eq!(ids_of(&last).len(), 1, "the last page carries the remainder: {:?}", last.body);
    let last_links = links(&last);
    assert!(
        !last_links.contains("rel=\"prev\"") && !last_links.contains("rel=\"next\""),
        "neither direction is pointed at on this binding, got: {last_links}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.10 gives a temporal response two independent pagination
/// dimensions: "Temporal representation of resources adds an additional
/// dimension to the pagination", answered with the "Partial Content Response
/// (206)" and a `Content-Range` whose `"'unit' shall be equal to 'DateTime'"` and,
/// under `lastN`, whose "'size' shall be equal to the requested lastN". That
/// bounds how much history each entity carries; the clause 4.12 `Link`
/// pagination bounds how many entities the page carries. Neither suppresses the
/// other, so a truncated page that still has entities left to traverse carries
/// both.
#[tokio::test]
async fn regression_temporal_pagination_content_range_and_links_travel_together() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=2).map(|index| scope.id(&format!("Vehicle:tem-range-{index}"))).collect();
    for id in &ids {
        create_temporal(client, &temporal_entity(id, "Vehicle", SYSTEM_LAST_N_CAP + 6)).await;
    }

    let response = get_query(
        client,
        &format!(
            "type=Vehicle&id={}&timerel=after&timeAt=2023-12-31T00:00:00Z&lastN=25&limit=1&offset=0",
            id_filter(&ids)
        ),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::PARTIAL_CONTENT,
        "clause 6.3.10: a history the broker cut short is a 206: {:?}",
        response.body
    );
    let range = response
        .headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .expect("a truncated page carries a Content-Range")
        .to_owned();
    assert!(range.starts_with("DateTime "), "clause 6.3.10 mandates the 'DateTime' range unit, got: {range}");
    assert_eq!(
        range.rsplit_once('/').expect("a Content-Range carries a size").1,
        "25",
        "clause 6.3.10: size shall equal the requested lastN, got: {range}"
    );

    assert_eq!(ids_of(&response).len(), 1, "the page is filled to the limit: {:?}", response.body);
    let page_links = links(&response);
    assert!(
        page_links.contains("rel=\"next\""),
        "the temporal-window dimension does not suppress the element dimension, got: {page_links}"
    );

    scope.finish().await.expect("cleanup failed");
}
