//! A federated Query Temporal Evolution of Entities over a Context Source whose
//! answer spans pages, and whose history a source cuts short.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.4.4: "The result of each remote query is an
//! Entity Array. The returned result is then merged into S4", paginated "as
//! mandated by clause 5.5.9", and clause 4.3.6.7 asks the broker to "initially
//! make a broad request to each registered Context Source" so that pagination is
//! consistent. The broker sent each source one `limit=1000` request and never
//! followed the source's `Link rel="next"` (clauses 5.5.9.1 and 6.3.10), so the
//! merged collection, its `NGSILD-Results-Count` and every hub page past the
//! first thousand entities stopped at 1,000 per source (real-world campaign
//! finding A-29, the temporal remainder of A-18).
//!
//! A source also pages each entity's history (clause 6.3.10): a 206 whose
//! `Content-Range` says where it stopped. Clause 6.3.10 gives the hub's response
//! one `Content-Range` its client resumes from, so the merged history is
//! complete only up to the earliest stop when paging forwards and down to the
//! latest when paging backwards; the hub answers 206 over that window and
//! leaves out what lies past it, rather than folding a partial source into a
//! 200.
//!
//! Each scenario releases its registration and its mock source before it
//! asserts anything, because a registration left behind points at a port that
//! no longer listens. The per-request ceiling (`federation.max_query_entities`,
//! `403 TooManyResults` under clause 4.12) is covered by the unit tests of
//! `andromeda-federation`'s source walk, since the shared test broker runs with
//! its default of 10,000.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
use andromeda_test_client::{entity::results_count, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, Utc};
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{ops::Range, str::FromStr};

/// The resource the forwarded temporal query lands on at the source.
const FORWARDED_QUERY_PATH: &str = "/ngsi-ld/v1/temporal/entities";
/// How many entities the source holds.
const REMOTE_ENTITIES: usize = 2_500;
/// The page size the source answers in.
const SOURCE_PAGE: usize = 1_000;
/// The source entities the hub holds as well, merged once (clause 4.5.5).
const SHARED: Range<usize> = 2_490..2_500;
/// Source positions after which the hub holds an entity of its own, so the
/// local entities interleave the source's across its page boundaries.
const INTERLEAVED: [usize; 4] = [0, 999, 1_000, 1_500];
/// The window every query of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2019-01-01T00:00:00Z";

/// The identifier of source entity `number` (or of the hub's interleaved
/// entity after it, with `suffix`).
fn identifier(scope: &TestEnvironment, number: usize, suffix: &str) -> String {
    scope.id(&format!("PagedTemporal:{number:05}{suffix}"))
}

/// The identifier-ordered union a hub query of the paged fixture answers.
fn union(scope: &TestEnvironment) -> Vec<String> {
    let mut ids: Vec<String> = (0..REMOTE_ENTITIES).map(|number| identifier(scope, number, "")).collect();
    ids.extend(INTERLEAVED.iter().map(|number| identifier(scope, *number, "L")));
    ids.sort_unstable();
    ids
}

/// One observation of `attribute` holding `value`, observed at `at`.
fn observation(value: &Value, at: &str) -> Value {
    json!([{"type": "Property", "value": value, "observedAt": at}])
}

/// One page of the source's temporal answer, starting at `first`, pointing at
/// the next page unless it is the last one.
async fn stub_source_page(source: &ContextSourceAdapter, scope: &TestEnvironment, entity_type: &str, first: usize) {
    let last = (first + SOURCE_PAGE).min(REMOTE_ENTITIES);
    let entities: Vec<Value> = (first..last)
        .map(|number| {
            json!({
                "id": identifier(scope, number, ""),
                "type": entity_type,
                "remote": observation(&json!(number), "2020-01-01T00:00:00Z"),
                "@context": TEST_SUITE_CONTEXT.as_str()
            })
        })
        .collect();
    let next = format!("<{FORWARDED_QUERY_PATH}?type={entity_type}&limit={SOURCE_PAGE}&offset={last}>; rel=\"next\"");
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

/// Register `source` for the temporal queries of `entity_type`, answering the
/// registration status.
async fn register(client: &TestClient, scope: &TestEnvironment, source: &ContextSourceAdapter, entity_type: &str) -> StatusCode {
    let registration = json!({
        "id": scope.id(&format!("registration:{entity_type}")),
        "type": "ContextSourceRegistration",
        "endpoint": source.url("/"),
        "mode": "inclusive",
        "operations": ["queryTemporal"],
        "information": [{"entities": [{"type": entity_type}]}],
        "@context": [TEST_SUITE_CONTEXT]
    });
    client.create_registration(&registration).await.expect("create registration").status
}

/// Create one Temporal Evolution on the hub, answering its status.
async fn create_local(client: &TestClient, body: Value) -> StatusCode {
    client.create_temporal_entity(&body).await.expect("create temporal entity request").status
}

/// Register the paged source and create the hub's own temporal entities,
/// answering every setup status.
async fn set_up_paged(client: &TestClient, scope: &TestEnvironment, source: &ContextSourceAdapter, entity_type: &str) -> Vec<StatusCode> {
    let mut statuses = vec![register(client, scope, source, entity_type).await];
    let local_ids = INTERLEAVED
        .iter()
        .map(|number| identifier(scope, *number, "L"))
        .chain(SHARED.map(|number| identifier(scope, number, "")));
    for id in local_ids {
        let body = json!({
            "id": id,
            "type": entity_type,
            "local": observation(&json!(true), "2020-01-01T00:00:00Z"),
            "@context": [TEST_SUITE_CONTEXT]
        });
        statuses.push(create_local(client, body).await);
    }
    statuses
}

/// One hub temporal query page.
async fn hub_page(client: &TestClient, query: &str) -> TestResponse {
    client
        .query_temporal_entities(query, Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("query temporal entities")
}

/// The entities a hub page answered with.
fn page_entities(response: &TestResponse) -> Vec<Value> {
    response.json::<Vec<Value>>().expect("a temporal query answers a JSON array")
}

/// The identifiers a hub page answered with.
fn page_ids(response: &TestResponse) -> Vec<String> {
    page_entities(response)
        .iter()
        .map(|entity| entity["id"].as_str().expect("every entity has an id").to_owned())
        .collect()
}

/// The query string of the page's `Link rel="<relation>"` target, if it has
/// one.
fn link_query(response: &TestResponse, relation: &str) -> Option<String> {
    let needle = format!("rel=\"{relation}\"");
    response
        .headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .find(|link| link.contains(&needle))
        .and_then(|link| {
            link.split_once('<')
                .and_then(|(_, rest)| rest.split_once('>'))
                .map(|(target, _)| target.split_once('?').map_or(target, |(_, query)| query).to_owned())
        })
}

/// The clause 6.3.10 `Content-Range` a partial response carries.
fn content_range(response: &TestResponse) -> Option<NgsiLdContentRange> {
    response
        .headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .map(|raw| NgsiLdContentRange::from_str(raw).expect("a clause 6.3.10 Content-Range"))
}

fn at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
}

/// The `observedAt` instants of every instance of `attribute` an entity carries.
fn observed_instants(entity: &Value, attribute: &str) -> Vec<String> {
    entity[attribute]
        .as_array()
        .map(|instances| {
            instances
                .iter()
                .filter_map(|instance| instance["observedAt"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
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

/// Clause 5.7.4.4 with clauses 5.5.9 and 6.3.13: every hub page -- at limit 100
/// and 1,000, at offsets inside, across and beyond the source's page
/// boundaries -- answers the exact window of the identifier-ordered union of
/// the local and the source's entities, and the count is the size of that union
/// with the shared entities counted once.
#[tokio::test]
async fn regression_federated_temporal_query_source_pagination_every_hub_page_is_the_window_of_the_whole_union() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedTemporalProbe");
    let source = ContextSourceAdapter::start().await;
    let windows: Vec<(usize, usize)> = vec![(100, 0), (100, 950), (100, 2_450), (1_000, 1_000), (1_000, 2_000)];

    let (setup, pages) = {
        let client = scope.client();
        let setup = set_up_paged(client, &scope, &source, &entity_type).await;
        stub_source_answers(&source, &scope, &entity_type, windows.len()).await;
        let mut pages = Vec::with_capacity(windows.len());
        for (limit, offset) in &windows {
            let query = format!("type={entity_type}&{WINDOW}&limit={limit}&offset={offset}&count=true");
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
    assert_eq!(union.len(), 2_504);
    for ((limit, offset), page) in windows.iter().zip(&pages) {
        assert_eq!(page.status, StatusCode::OK, "{}", page.query);
        let expected: Vec<String> = union.iter().skip(*offset).take(*limit).cloned().collect();
        assert_eq!(page.ids, expected, "the window of {}", page.query);
        assert_eq!(page.count, Some(union.len()), "the count of {}", page.query);
    }
}

/// Clause 6.3.10: following the hub's own `next` pointers from the first page
/// walks the whole union exactly once, and the last page carries no pointer.
///
/// Clause 5.5.9.1: the pointers are opaque cursors even though a source
/// contributes, because the merged collection is paged in the same identifier
/// order as the local one (clause 4.23.1 withholds only a client-requested
/// Entity ordering from distributed operations).
#[tokio::test]
async fn regression_federated_temporal_query_source_pagination_the_hub_next_pointer_walks_the_whole_union() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedTemporalWalkProbe");
    let source = ContextSourceAdapter::start().await;

    let (setup, walked, pointers) = {
        let client = scope.client();
        let setup = set_up_paged(client, &scope, &source, &entity_type).await;
        stub_source_answers(&source, &scope, &entity_type, 5).await;
        let mut walked = Vec::new();
        let mut pointers = Vec::new();
        let mut query = Some(format!("type={entity_type}&{WINDOW}&limit=1000"));
        while let Some(current) = query.take() {
            let response = hub_page(client, &current).await;
            assert_eq!(response.status, StatusCode::OK, "{current}");
            walked.extend(page_ids(&response));
            query = link_query(&response, "next");
            pointers.extend(query.clone());
            if pointers.len() > 5 {
                break;
            }
        }
        (setup, walked, pointers)
    };
    source.shutdown().await;
    let union = union(&scope);
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    assert_eq!(pointers.len(), 2, "2,504 entities at 1,000 per page are three pages");
    for pointer in &pointers {
        assert!(
            pointer.contains("cursor=") && !pointer.contains("offset="),
            "the hub points by cursor: {pointer}"
        );
    }
    assert_eq!(walked, union, "the walk answers every entity exactly once, in identifier order");
}

/// Clause 5.5.9.1: from the hub's `last` page, `prev` walks back through the
/// merged union page by page, and the short first page offers no further
/// `prev`.
#[tokio::test]
async fn regression_federated_temporal_query_source_pagination_last_then_prev_pages_back_through_the_union() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedTemporalBackwardProbe");
    let source = ContextSourceAdapter::start().await;

    let (setup, pages, first_offers_prev) = {
        let client = scope.client();
        let setup = set_up_paged(client, &scope, &source, &entity_type).await;
        stub_source_answers(&source, &scope, &entity_type, 4).await;
        let opening = hub_page(client, &format!("type={entity_type}&{WINDOW}&limit=1000")).await;
        let mut pages = Vec::new();
        let mut query = link_query(&opening, "last");
        let mut first_offers_prev = true;
        while let Some(current) = query.take() {
            let response = hub_page(client, &current).await;
            assert_eq!(response.status, StatusCode::OK, "{current}");
            pages.push(page_ids(&response));
            query = link_query(&response, "prev");
            first_offers_prev = query.is_some();
            if pages.len() > 4 {
                break;
            }
        }
        (setup, pages, first_offers_prev)
    };
    source.shutdown().await;
    let union = union(&scope);
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    let expected: Vec<Vec<String>> = vec![union[1_504..].to_vec(), union[504..1_504].to_vec(), union[..504].to_vec()];
    assert_eq!(pages, expected, "the final page, then each page before it");
    assert!(!first_offers_prev, "the short first page offers no prev");
}

/// Clause 4.5.5: an entity both the hub and the source hold is one entity of
/// the union, carrying the history of both.
#[tokio::test]
async fn regression_federated_temporal_query_source_pagination_an_entity_held_on_both_sides_is_merged_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedTemporalMergeProbe");
    let source = ContextSourceAdapter::start().await;

    let (setup, response) = {
        let client = scope.client();
        let setup = set_up_paged(client, &scope, &source, &entity_type).await;
        stub_source_answers(&source, &scope, &entity_type, 1).await;
        let response = hub_page(client, &format!("type={entity_type}&{WINDOW}&limit=20&offset=2490&count=true")).await;
        (setup, response)
    };
    source.shutdown().await;
    let shared: Vec<String> = SHARED.map(|number| identifier(&scope, number, "")).collect();
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    assert_eq!(response.status, StatusCode::OK);
    let entities = page_entities(&response);
    for id in &shared {
        let matching: Vec<&Value> = entities.iter().filter(|entity| entity["id"] == json!(id)).collect();
        assert_eq!(matching.len(), 1, "{id} appears once");
        assert_eq!(
            observed_instants(matching[0], "local").len(),
            1,
            "{id} keeps the hub's history: {}",
            matching[0]
        );
        assert_eq!(
            observed_instants(matching[0], "remote").len(),
            1,
            "{id} carries the source's history: {}",
            matching[0]
        );
    }
    assert_eq!(results_count(&response.headers), Some(2_504));
}

/// Clause 6.3.17: a source page that fails keeps what the earlier pages
/// gathered and says so with an `NGSILD-Warning`, rather than failing the query
/// or pretending the answer was complete.
#[tokio::test]
async fn regression_federated_temporal_query_source_pagination_a_failing_source_page_keeps_the_gathered_entities_with_a_warning() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PagedTemporalFailureProbe");
    let source = ContextSourceAdapter::start().await;

    let (setup, response) = {
        let client = scope.client();
        let setup = set_up_paged(client, &scope, &source, &entity_type).await;
        stub_source_page(&source, &scope, &entity_type, 0).await;
        source.set_stub(Method::GET, FORWARDED_QUERY_PATH, 500).await;
        let response = hub_page(client, &format!("type={entity_type}&{WINDOW}&limit=1000&count=true")).await;
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

/// The hub's own entity for the partial-history scenarios: `speed` observed
/// on the 1st and on the 3rd of January 2020.
fn local_speed(id: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "speed": [
            {"type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z"},
            {"type": "Property", "value": 3, "observedAt": "2020-01-03T00:00:00Z"}
        ],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// The source's entity for the partial-history scenarios: one `speed`
/// observed on the 2nd of January 2020, answered as a 206 cut at `range`.
async fn stub_partial_source(source: &ContextSourceAdapter, id: &str, entity_type: &str, range: &str) {
    let entities = json!([{
        "id": id,
        "type": entity_type,
        "speed": observation(&json!(2), "2020-01-02T00:00:00Z"),
        "@context": TEST_SUITE_CONTEXT.as_str()
    }]);
    source
        .set_stub_with_body_and_headers(Method::GET, FORWARDED_QUERY_PATH, 206, &entities, &[("content-range", range)])
        .await;
}

/// Clause 6.3.10 forwards: the source stopped at the 2nd, so the hub's page is
/// complete only up to it. The hub answers 206 with that range-end, leaves out
/// its own later instance, and a request resumed from the range-end reaches it.
#[tokio::test]
async fn regression_federated_temporal_query_source_pagination_a_source_cut_short_forwards_narrows_the_hub_history() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PartialTemporalProbe");
    let source = ContextSourceAdapter::start().await;
    let local_id = scope.id(&format!("{entity_type}:local"));
    let remote_id = scope.id(&format!("{entity_type}:remote"));

    let (setup, first, resumed) = {
        let client = scope.client();
        let setup = vec![
            register(client, &scope, &source, &entity_type).await,
            create_local(client, local_speed(&local_id, &entity_type)).await,
        ];
        stub_partial_source(&source, &remote_id, &entity_type, "DateTime 2019-01-01T00:00:00Z-2020-01-02T00:00:00Z/*").await;
        source
            .set_stub_with_body(Method::GET, FORWARDED_QUERY_PATH, 200, &Value::Array(Vec::new()))
            .await;
        let first = hub_page(client, &format!("type={entity_type}&{WINDOW}")).await;
        let resumed = hub_page(client, &format!("type={entity_type}&timerel=after&timeAt=2020-01-02T00:00:00Z")).await;
        (setup, first, resumed)
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    assert_eq!(
        first.status,
        StatusCode::PARTIAL_CONTENT,
        "a partial source makes a partial response: {:?}",
        first.body
    );
    let range = content_range(&first).expect("a 206 carries a Content-Range");
    assert_eq!(range.start(), at("2019-01-01T00:00:00Z"));
    assert_eq!(range.end(), at("2020-01-02T00:00:00Z"), "the range ends where the source stopped");
    let entities = page_entities(&first);
    let by_id = |id: &str| entities.iter().find(|entity| entity["id"] == json!(id)).expect("the entity is in the page");
    assert_eq!(observed_instants(by_id(&local_id), "speed"), vec!["2020-01-01T00:00:00Z".to_owned()]);
    assert_eq!(observed_instants(by_id(&remote_id), "speed"), vec!["2020-01-02T00:00:00Z".to_owned()]);

    assert_eq!(resumed.status, StatusCode::OK, "{:?}", resumed.body);
    let resumed = page_entities(&resumed);
    let local = resumed
        .iter()
        .find(|entity| entity["id"] == json!(local_id))
        .expect("the hub's entity continues");
    assert!(
        observed_instants(local, "speed").contains(&"2020-01-03T00:00:00Z".to_owned()),
        "the instance left out reaches the client on the resumed request: {local}"
    );
}

/// Clause 6.3.10 backwards (with `lastN`): the source stopped at the 2nd going
/// back, so the hub's page is complete only down to it. The hub answers 206
/// with that range-end and the requested `lastN` as size, and leaves out its
/// own earlier instance.
#[tokio::test]
async fn regression_federated_temporal_query_source_pagination_a_source_cut_short_backwards_narrows_the_hub_history() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PartialTemporalBackProbe");
    let source = ContextSourceAdapter::start().await;
    let local_id = scope.id(&format!("{entity_type}:local"));
    let remote_id = scope.id(&format!("{entity_type}:remote"));

    let (setup, response) = {
        let client = scope.client();
        let setup = vec![
            register(client, &scope, &source, &entity_type).await,
            create_local(client, local_speed(&local_id, &entity_type)).await,
        ];
        stub_partial_source(&source, &remote_id, &entity_type, "DateTime 2020-01-02T00:00:00Z-2020-01-02T00:00:00Z/5").await;
        let response = hub_page(client, &format!("type={entity_type}&{WINDOW}&lastN=5")).await;
        (setup, response)
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    for status in setup {
        assert!(status.is_success(), "a setup request failed: {status}");
    }
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "{:?}", response.body);
    let range = content_range(&response).expect("a 206 carries a Content-Range");
    assert_eq!(range.end(), at("2020-01-02T00:00:00Z"), "the range ends where the source stopped going back");
    assert_eq!(range.total(), Some(5), "clause 6.3.10: size is the requested lastN");
    let entities = page_entities(&response);
    let local = entities
        .iter()
        .find(|entity| entity["id"] == json!(local_id))
        .expect("the hub's entity is in the page");
    assert_eq!(observed_instants(local, "speed"), vec!["2020-01-03T00:00:00Z".to_owned()]);
}
