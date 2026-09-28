//! A federated Query Entities over a Context Source whose answer spans pages.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.2.4 makes the result of a federated query
//! the local entities merged "with the locally queried result according to the
//! algorithm defined in clause 4.5.5", paginated "as mandated by clause 5.5.9",
//! and clause 4.3.6.7 asks the broker to "initially make a broad request to each
//! registered Context Source" so that pagination is consistent. The broker sent
//! each source one `limit=1000` request and never followed the source's
//! `Link rel="next"` (clauses 5.5.9.1 and 6.3.10), so the merged collection, its
//! `NGSILD-Results-Count` and every hub page past the first thousand entities
//! stopped at 1,000: a live GTFS-RT feed with 3,040 vehicles on the source
//! answered 1,000 through the hub, and offsets 1,000 and 2,000 answered nothing.
//!
//! Every scenario registers a mock Context Source serving 2,500 entities in
//! pages of 1,000 linked by `next`, beside entities the hub holds itself --
//! some interleaving the source's identifiers, some the same entities the
//! source holds -- and asserts each hub page against the identifier-ordered
//! union. Each scenario releases its registration and its mock source before it
//! asserts anything, because a registration left behind points at a port that
//! no longer listens.
//!
//! The per-request ceiling (`federation.max_query_entities`, answered with
//! `403 TooManyResults` under clause 4.12) is a broker-wide setting the shared
//! test broker runs with its default of 10,000, so it is covered by the unit
//! tests of `andromeda-federation`'s source walk rather than here.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::{entity::results_count, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::ops::Range;

/// The resource the forwarded query lands on at the source.
const FORWARDED_QUERY_PATH: &str = "/ngsi-ld/v1/entities";
/// How many entities the source holds.
const REMOTE_ENTITIES: usize = 2_500;
/// The page size the source answers in.
const SOURCE_PAGE: usize = 1_000;
/// The source entities the hub holds as well, merged once (clause 4.5.5).
const SHARED: Range<usize> = 2_490..2_500;
/// Source positions after which the hub holds an entity of its own, so the
/// local entities interleave the source's across its page boundaries.
const INTERLEAVED: [usize; 4] = [0, 999, 1_000, 1_500];

/// The identifier of source entity `number` (or of the hub's interleaved
/// entity after it, with `suffix`).
fn identifier(scope: &TestEnvironment, number: usize, suffix: &str) -> String {
    scope.id(&format!("PagedVehicle:{number:05}{suffix}"))
}

/// The identifier-ordered union a hub query of the fixture answers.
fn union(scope: &TestEnvironment) -> Vec<String> {
    let mut ids: Vec<String> = (0..REMOTE_ENTITIES).map(|number| identifier(scope, number, "")).collect();
    ids.extend(INTERLEAVED.iter().map(|number| identifier(scope, *number, "L")));
    ids.sort_unstable();
    ids
}

/// One page of the source's answer, starting at `first`, pointing at the next
/// page unless it is the last one.
async fn stub_source_page(source: &ContextSourceAdapter, scope: &TestEnvironment, entity_type: &str, first: usize) {
    let last = (first + SOURCE_PAGE).min(REMOTE_ENTITIES);
    let entities: Vec<Value> = (first..last)
        .map(|number| {
            json!({
                "id": identifier(scope, number, ""),
                "type": entity_type,
                "remote": {"type": "Property", "value": number},
                "@context": TEST_SUITE_CONTEXT.as_str()
            })
        })
        .collect();
    let next = format!("</ngsi-ld/v1/entities?type={entity_type}&limit={SOURCE_PAGE}&offset={last}>; rel=\"next\"");
    let headers: Vec<(&str, &str)> = if last < REMOTE_ENTITIES { vec![("link", next.as_str())] } else { Vec::new() };
    source
        .set_stub_with_body_and_headers(Method::GET, FORWARDED_QUERY_PATH, 200, &Value::Array(entities), &headers)
        .await;
}

/// Queue the source's whole answer, once for each of `walks` hub queries.
async fn stub_source_answers(source: &ContextSourceAdapter, scope: &TestEnvironment, entity_type: &str, walks: usize) {
    for _ in 0..walks {
        for first in (0..REMOTE_ENTITIES).step_by(SOURCE_PAGE) {
            stub_source_page(source, scope, entity_type, first).await;
        }
    }
}

/// Register the source for `entity_type` and create the hub's own entities,
/// answering every setup status.
async fn set_up(client: &TestClient, scope: &TestEnvironment, source: &ContextSourceAdapter, entity_type: &str) -> Vec<StatusCode> {
    let registration = json!({
        "id": scope.id("registration:paged-source"),
        "type": "ContextSourceRegistration",
        "endpoint": source.url("/"),
        "mode": "inclusive",
        "operations": ["federationOps"],
        "information": [{"entities": [{"type": entity_type}]}],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let registered = client.create_registration(&registration).await.expect("create registration").status;
    let local: Vec<Value> = INTERLEAVED
        .iter()
        .map(|number| identifier(scope, *number, "L"))
        .chain(SHARED.map(|number| identifier(scope, number, "")))
        .map(|id| json!({"id": id, "type": entity_type, "local": {"type": "Property", "value": true}, "@context": TEST_SUITE_CONTEXT.as_str()}))
        .collect();
    let created = client.batch_create(&Value::Array(local)).await.expect("batch create").status;
    vec![registered, created]
}

/// One hub query page.
async fn hub_page(client: &TestClient, query: &str) -> TestResponse {
    client
        .query_entities(query, "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("query entities")
}

/// The identifiers a hub page answered with.
fn page_ids(response: &TestResponse) -> Vec<String> {
    response
        .json_value()
        .expect("a JSON page")
        .as_array()
        .expect("a query answers an array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("every entity has an id").to_owned())
        .collect()
}

/// The target of the page's `Link rel="next"`, if it has one.
fn next_link(response: &TestResponse) -> Option<String> {
    response
        .headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .find(|link| link.contains("rel=\"next\""))
        .and_then(|link| {
            link.split_once('<')
                .and_then(|(_, rest)| rest.split_once('>'))
                .map(|(target, _)| target.to_owned())
        })
}

/// What one hub page answered, kept so every assertion runs after the
/// registration and the mock source are gone.
struct HubPage {
    /// The query the page was asked with.
    query: String,
    /// The status it answered with.
    status: StatusCode,
    /// The identifiers it answered with.
    ids: Vec<String>,
    /// The `NGSILD-Results-Count` it reported.
    count: Option<usize>,
}

/// Clause 5.7.2.4 with clauses 5.5.9 and 6.3.13: every hub page -- at limit 100
/// and 1,000, at offsets inside, across and beyond the source's page
/// boundaries -- answers the exact window of the identifier-ordered union of
/// the local and the source's entities, and the count is the size of that union
/// with the shared entities counted once.
#[tokio::test]
async fn regression_federated_query_source_pagination_every_hub_page_is_the_window_of_the_whole_union() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedSourceProbe");
    let source = ContextSourceAdapter::start().await;
    let windows: Vec<(usize, usize)> = vec![
        (100, 0),
        (100, 900),
        (100, 950),
        (100, 1_000),
        (100, 1_950),
        (100, 2_450),
        (100, 2_500),
        (1_000, 0),
        (1_000, 1_000),
        (1_000, 2_000),
    ];

    let (setup, pages) = {
        let client = scope.client();
        let setup = set_up(client, &scope, &source, &entity_type).await;
        stub_source_answers(&source, &scope, &entity_type, windows.len()).await;
        let mut pages = Vec::with_capacity(windows.len());
        for (limit, offset) in &windows {
            let query = format!("type={entity_type}&limit={limit}&offset={offset}&count=true");
            let response = hub_page(client, &query).await;
            pages.push(HubPage {
                status: response.status,
                ids: if response.status == StatusCode::OK { page_ids(&response) } else { Vec::new() },
                count: results_count(&response.headers),
                query,
            });
        }
        (setup, pages)
    };
    source.shutdown().await;
    let union = union(&scope);
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    pretty_assertions::assert_eq!(union.len(), 2_504);
    for ((limit, offset), page) in windows.iter().zip(&pages) {
        assert_eq!(page.status, StatusCode::OK, "{}", page.query);
        let expected: Vec<String> = union.iter().skip(*offset).take(*limit).cloned().collect();
        assert_eq!(page.ids, expected, "the window of {}", page.query);
        assert_eq!(page.count, Some(union.len()), "the count of {}", page.query);
    }
}

/// Clause 6.3.10: following the hub's own `next` pointers from the first page
/// walks the whole union exactly once, and the last page carries no pointer.
#[tokio::test]
async fn regression_federated_query_source_pagination_the_hub_next_pointer_walks_the_whole_union() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedSourceWalkProbe");
    let source = ContextSourceAdapter::start().await;

    let (setup, walked, pages) = {
        let client = scope.client();
        let setup = set_up(client, &scope, &source, &entity_type).await;
        stub_source_answers(&source, &scope, &entity_type, 5).await;
        let mut walked = Vec::new();
        let mut pages = 0_usize;
        let mut query = Some(format!("type={entity_type}&limit=1000"));
        while let Some(current) = query.take() {
            let response = hub_page(client, &current).await;
            assert_eq!(response.status, StatusCode::OK, "{current}");
            walked.extend(page_ids(&response));
            pages += 1;
            query = next_link(&response).map(|target| target.split_once('?').map(|(_, query)| query.to_owned()).unwrap_or(target));
            if pages > 5 {
                break;
            }
        }
        (setup, walked, pages)
    };
    source.shutdown().await;
    let union = union(&scope);
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    assert_eq!(pages, 3, "2,504 entities at 1,000 per page");
    assert_eq!(walked, union, "the walk answers every entity exactly once, in identifier order");
}

/// Clause 4.5.5: an entity both the hub and the source hold is one entity of
/// the union, carrying the attributes of both.
#[tokio::test]
async fn regression_federated_query_source_pagination_an_entity_held_on_both_sides_is_merged_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedSourceMergeProbe");
    let source = ContextSourceAdapter::start().await;

    let (setup, response) = {
        let client = scope.client();
        let setup = set_up(client, &scope, &source, &entity_type).await;
        stub_source_answers(&source, &scope, &entity_type, 1).await;
        let response = hub_page(client, &format!("type={entity_type}&limit=20&offset=2490&count=true")).await;
        (setup, response)
    };
    source.shutdown().await;
    let shared: Vec<String> = SHARED.map(|number| identifier(&scope, number, "")).collect();
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    assert_eq!(response.status, StatusCode::OK);
    let entities = response.json_value().expect("a JSON page");
    let entities = entities.as_array().expect("an array");
    for id in &shared {
        let matching: Vec<&Value> = entities.iter().filter(|entity| entity["id"] == json!(id)).collect();
        assert_eq!(matching.len(), 1, "{id} appears once");
        assert_eq!(matching[0]["local"]["value"], json!(true), "{id} keeps the hub's attribute");
        assert!(
            matching[0]["remote"]["value"].is_number(),
            "{id} carries the source's attribute: {}",
            matching[0]
        );
    }
    assert_eq!(results_count(&response.headers), Some(2_504));
}

/// Clause 6.3.17: a source page that fails keeps what the earlier pages
/// gathered and says so with an `NGSILD-Warning`, rather than failing the query
/// or pretending the answer was complete.
#[tokio::test]
async fn regression_federated_query_source_pagination_a_failing_source_page_keeps_the_gathered_entities_with_a_warning() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedSourceFailureProbe");
    let source = ContextSourceAdapter::start().await;

    let (setup, response) = {
        let client = scope.client();
        let setup = set_up(client, &scope, &source, &entity_type).await;
        stub_source_page(&source, &scope, &entity_type, 0).await;
        source.set_stub(Method::GET, FORWARDED_QUERY_PATH, 500).await;
        let response = hub_page(client, &format!("type={entity_type}&limit=1000&count=true")).await;
        (setup, response)
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    assert_eq!(response.status, StatusCode::OK, "a failing source page does not fail the query");
    // The first source page (1,000 entities) and the hub's own 14, of which 10
    // are held by the source beyond its first page and so stand alone.
    assert_eq!(results_count(&response.headers), Some(1_014));
    assert!(
        response.headers.contains_key("ngsild-warning"),
        "the incomplete source answer is reported: {:?}",
        response.headers
    );
}
