//! Collection pagination under each pagination policy an operator can choose.
//!
//! The base tier's broker runs the default policy (`[pagination] default_mode
//! = "cursor"`, `[extensions] cursor_pagination = "enabled"`). Every other
//! policy needs a broker of its own, so each test here starts the binary under
//! test with the settings it exercises, against the tier's database, and pages
//! a collection of entities of a type no other test writes (clauses 4.12,
//! 5.5.9.1, 5.5.9.2 and 6.3.10).
//!
//! Naming convention: `<policy>_broker_<behaviour>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::problem_detail::INVALID_REQUEST;
use andromeda_test_harness::broker_process::BrokerProcess;
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

/// The binary under test.
fn binary() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_andromeda"))
}

/// An entity type no other test writes, so a query by it sees this test's
/// entities alone.
fn unique_type() -> String {
    format!("PolicyItem{}", Uuid::new_v4().simple())
}

/// Create `count` entities of `entity_type`, returning their identifiers in
/// identifier order.
async fn create_items(http: &Client, broker: &BrokerProcess, entity_type: &str, count: u32) -> Vec<String> {
    let mut ids: Vec<String> = (0..count).map(|index| format!("urn:ngsi-ld:{entity_type}:{index:04}")).collect();
    for id in &ids {
        let response = http
            .post(broker.url("entities"))
            .header("Content-Type", "application/json")
            .body(json!({"id": id, "type": entity_type, "name": {"type": "Property", "value": "p"}}).to_string())
            .send()
            .await
            .expect("create entity request");
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    ids.sort();
    ids
}

/// Remove the entities of `entity_type`, leaving the shared database as found.
async fn purge_items(http: &Client, broker: &BrokerProcess, entity_type: &str) {
    let response = http
        .delete(format!("{}?type={entity_type}", broker.url("entities")))
        .send()
        .await
        .expect("purge request");
    assert!(response.status().is_success(), "purge: {}", response.status());
}

/// One entity query: its status, headers and body.
async fn query(http: &Client, broker: &BrokerProcess, query: &str) -> (StatusCode, HeaderMap, Value) {
    let response = http
        .get(format!("{}?{query}", broker.url("entities")))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("query request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, headers, body)
}

/// Every `Link` header value the response carries, joined for inspection.
fn links(headers: &HeaderMap) -> String {
    headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The query string of the `rel="<rel>"` pagination link, if any.
fn link_query(headers: &HeaderMap, rel: &str) -> Option<String> {
    let needle = format!("rel=\"{rel}\"");
    links(headers).split(',').find(|value| value.contains(&needle)).and_then(|value| {
        let start = value.find('<')? + 1;
        let end = value[start..].find('>')? + start;
        value[start..end].split_once('?').map(|(_, query)| query.to_owned())
    })
}

/// The `id` of every entity in a collection body, in order.
fn ids_of(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("a collection body")
        .iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Follow `rel="next"` from `start` to exhaustion, collecting every id.
async fn walk(http: &Client, broker: &BrokerProcess, start: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let mut next = Some(start.to_owned());
    while let Some(current) = next {
        let (status, headers, body) = query(http, broker, &current).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        seen.extend(ids_of(&body));
        next = link_query(&headers, "next");
        assert!(seen.len() <= 100, "the walk does not loop");
    }
    seen
}

/// `[pagination] default_mode = "offset"`: a fresh query is paged by offset.
#[tokio::test]
async fn offset_default_broker_hands_out_offset_links() {
    let broker = BrokerProcess::start(binary(), &[("ANDROMEDA__PAGINATION__DEFAULT_MODE", "offset")])
        .await
        .expect("the broker starts");
    let http = Client::new();
    let entity_type = unique_type();
    create_items(&http, &broker, &entity_type, 3).await;

    let (status, headers, _) = query(&http, &broker, &format!("type={entity_type}&limit=2")).await;
    assert_eq!(status, StatusCode::OK);
    let next = link_query(&headers, "next").expect("a full first page has a next");
    assert!(next.contains("offset=2"), "{next}");
    assert!(!links(&headers).contains("cursor="), "{}", links(&headers));
    purge_items(&http, &broker, &entity_type).await;
}

/// Under an offset default an empty `cursor=` still opts into a cursor walk,
/// which covers every entity once (clause 5.5.9.1).
#[tokio::test]
async fn offset_default_broker_honours_an_empty_cursor_opt_in() {
    let broker = BrokerProcess::start(binary(), &[("ANDROMEDA__PAGINATION__DEFAULT_MODE", "offset")])
        .await
        .expect("the broker starts");
    let http = Client::new();
    let entity_type = unique_type();
    let ids = create_items(&http, &broker, &entity_type, 5).await;

    let (status, headers, _) = query(&http, &broker, &format!("type={entity_type}&limit=2&cursor=")).await;
    assert_eq!(status, StatusCode::OK);
    let pointers = links(&headers);
    assert!(pointers.contains("cursor="), "{pointers}");
    assert!(!pointers.contains("offset="), "{pointers}");
    assert_eq!(walk(&http, &broker, &format!("type={entity_type}&limit=2&cursor=")).await, ids);
    purge_items(&http, &broker, &entity_type).await;
}

/// `[extensions] cursor_pagination = "disabled"` with an offset default:
/// offsets only, and a `cursor` is a parameter the broker does not offer.
#[tokio::test]
async fn offset_only_broker_refuses_a_cursor() {
    let broker = BrokerProcess::start(
        binary(),
        &[
            ("ANDROMEDA__EXTENSIONS__CURSOR_PAGINATION", "disabled"),
            ("ANDROMEDA__PAGINATION__DEFAULT_MODE", "offset"),
        ],
    )
    .await
    .expect("the broker starts");
    let http = Client::new();
    let entity_type = unique_type();
    create_items(&http, &broker, &entity_type, 3).await;

    let (status, _, body) = query(&http, &broker, &format!("type={entity_type}&limit=2&cursor=")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body.get("type").and_then(Value::as_str), Some(INVALID_REQUEST.as_str()), "{body}");

    let (status, headers, _) = query(&http, &broker, &format!("type={entity_type}&limit=2")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        link_query(&headers, "next").is_some_and(|next| next.contains("offset=2")),
        "{}",
        links(&headers)
    );
    purge_items(&http, &broker, &entity_type).await;
}

/// Conformance mode forces every extension off, cursor pagination included,
/// so the broker pages by offset, refuses a cursor and advertises nothing.
#[tokio::test]
async fn conformance_mode_broker_pages_by_offset() {
    let broker = BrokerProcess::start(binary(), &[("ANDROMEDA__EXTENSIONS__CONFORMANCE_MODE", "enabled")])
        .await
        .expect("the broker starts");
    let http = Client::new();
    let entity_type = unique_type();
    create_items(&http, &broker, &entity_type, 3).await;

    let (status, headers, _) = query(&http, &broker, &format!("type={entity_type}&limit=2")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        link_query(&headers, "next").is_some_and(|next| next.contains("offset=2")),
        "{}",
        links(&headers)
    );
    let (status, _, _) = query(&http, &broker, &format!("type={entity_type}&limit=2&cursor=")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let identity: Value = http
        .get(broker.url("info/sourceIdentity"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("identity request")
        .json()
        .await
        .expect("an identity body");
    assert!(identity.pointer("/contextSourceExtras/extensions").is_none(), "{identity}");
    purge_items(&http, &broker, &entity_type).await;
}

/// Disabling cursor pagination while keeping cursors the default mode is a
/// contradiction the broker refuses to start with.
#[tokio::test]
async fn cursor_off_with_cursor_default_refuses_to_start() {
    let refused = BrokerProcess::refused_start(binary(), &[("ANDROMEDA__EXTENSIONS__CURSOR_PAGINATION", "disabled")])
        .await
        .expect("the broker process is observable");
    assert!(!refused.status.success(), "the broker must not start: {}", refused.log);
    assert!(
        refused.log.contains("pagination.default_mode"),
        "the log names the configuration error: {}",
        refused.log
    );
}
