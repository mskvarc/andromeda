//! The temporal representation of an attribute renders a client-supplied
//! `observedAt` at the sub-second precision it was stored with, rather than
//! truncating it to whole seconds.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 defines `DateTime` with a fractional
//! second of up to six digits, and PostgreSQL `TIMESTAMPTZ` stores exactly that
//! microsecond resolution. The temporal store keeps the instant, not the
//! client's lexical spelling, and the temporal evolution retrieval of clauses
//! 5.7.3/5.7.4 renders it keeping exactly the sub-second precision it holds: a
//! write of `...:00.123Z` reads back `...:00.123Z`, so a returned `observedAt`
//! can be reused as an exact bound in a later temporal query and two instances
//! less than a second apart stay distinct.
//!
//! The defect truncated every temporal `observedAt` to whole seconds, dropping
//! the sub-second precision entirely.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The first observation instant the client writes, carrying milliseconds.
const FIRST_OBSERVED_AT: &str = "2026-03-01T10:00:00.123Z";
/// The second observation instant the client writes, one and a bit seconds
/// later, also with milliseconds.
const SECOND_OBSERVED_AT: &str = "2026-03-01T10:00:01.456Z";
/// The first instant as the temporal API renders it back. A millisecond value
/// renders three fractional digits, so it is the identical string the client
/// wrote (clause 4.6.3).
const FIRST_OBSERVED_AT_RENDERED: &str = "2026-03-01T10:00:00.123Z";
/// The second instant as the temporal API renders it back.
const SECOND_OBSERVED_AT_RENDERED: &str = "2026-03-01T10:00:01.456Z";

/// Create one temporal entity carrying two `p` instances observed a fraction of
/// a second apart, and return its id.
async fn create_sub_second_entity(client: &TestClient, scope: &TestEnvironment) -> String {
    let id = scope.id("T:observed-at-sub-second");
    let body = json!({
        "id": id,
        "type": "T",
        "p": [
            {"type": "Property", "value": 1, "observedAt": FIRST_OBSERVED_AT},
            {"type": "Property", "value": 2, "observedAt": SECOND_OBSERVED_AT}
        ],
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_temporal_entity(&body).await.expect("create temporal entity request");
    assert!(
        created.status == StatusCode::CREATED || created.status == StatusCode::NO_CONTENT,
        "create temporal entity: {:?}",
        created.body
    );
    id
}

/// The `observedAt` the `p` instance whose value is `value` reports, from a
/// temporal retrieval body.
fn observed_at_of(body: &Value, value: i64) -> String {
    let instances = body["p"].as_array().expect("the temporal representation carries the p instances");
    let instance = instances
        .iter()
        .find(|instance| instance["value"] == json!(value))
        .expect("an instance with the requested value");
    instance["observedAt"].as_str().expect("the instance carries an observedAt").to_owned()
}

/// Clauses 5.7.3/5.7.4 with 4.6.3: a temporal retrieval hands back the exact
/// sub-second `observedAt` the client wrote, for every instance.
#[tokio::test]
async fn regression_temporal_observed_at_keeps_its_milliseconds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_sub_second_entity(client, &scope).await;

    let response = client
        .get_temporal_entity(&id, "timerel=after&timeAt=2026-03-01T00:00:00Z", Some(CORE_CONTEXT))
        .await
        .expect("temporal retrieval request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieval: {:?}", response.body);
    let body = response.json_value().expect("a temporal entity body");

    assert_eq!(
        observed_at_of(&body, 1),
        FIRST_OBSERVED_AT_RENDERED,
        "the first instance keeps its milliseconds rather than truncating to whole seconds: {body}"
    );
    assert_eq!(
        observed_at_of(&body, 2),
        SECOND_OBSERVED_AT_RENDERED,
        "the second instance keeps its milliseconds: {body}"
    );

    client.delete_temporal_entity(&id).await.expect("delete temporal entity request");
    scope.finish().await.expect("environment cleanup");
}

/// A sub-second window that lies strictly between the two observations selects
/// only the first instance, so a returned `observedAt` is precise enough to be
/// used as an exact bound in a later query.
#[tokio::test]
async fn regression_temporal_sub_second_window_selects_one_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_sub_second_entity(client, &scope).await;

    // The window's bounds carry milliseconds and bracket only the first
    // observation (`...:00.123Z`), excluding the second (`...:01.456Z`).
    let response = client
        .get_temporal_entity(
            &id,
            "timerel=between&timeAt=2026-03-01T10:00:00.050Z&endTimeAt=2026-03-01T10:00:00.200Z",
            Some(CORE_CONTEXT),
        )
        .await
        .expect("temporal window request");
    assert_eq!(response.status, StatusCode::OK, "temporal window retrieval: {:?}", response.body);
    let body = response.json_value().expect("a temporal entity body");

    let instances = body["p"].as_array().expect("the windowed representation carries the p instances");
    assert_eq!(instances.len(), 1, "the sub-second window matches exactly the first instance: {body}");
    assert_eq!(instances[0]["value"], json!(1), "the matched instance is the first observation: {body}");
    assert_eq!(
        instances[0]["observedAt"].as_str(),
        Some(FIRST_OBSERVED_AT_RENDERED),
        "the matched instance reports its stored sub-second observedAt: {body}"
    );

    client.delete_temporal_entity(&id).await.expect("delete temporal entity request");
    scope.finish().await.expect("environment cleanup");
}
