//! A temporal query naming no window is bad request data.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::problem,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::SnapshotId,
};

/// Posts `body` as written to Create Snapshot and expects `BadRequestData` naming `needle` in the answer.
async fn expect_refused(broker: &BrokerClient, body: &str, needle: &str) -> Result<()> {
    // The temporal query breaks a rule the typed query refuses, so the body is written as text.
    let request = json_request_to(broker, Method::POST, "snapshots", body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing(needle)
    );
    Ok(())
}

/// Asserts that no Snapshot `id` exists.
async fn expect_absent(broker: &BrokerClient, id: &SnapshotId) {
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
}

/// Pins: Create Snapshot whose `snapshotTemporalQueries` element carries a `temporalQ` between without `endTimeAt`,
/// before with an `endTimeAt`, or between closing before its `timeAt` answers 400 `BadRequestData` naming
/// `endTimeAt`, and no Snapshot exists
/// Clause: 4.11, 5.2.21, Table 5.2.21-1, 5.16.1.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8156_a_temporal_query_naming_no_window_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let temporal_queries = [
        ("no-end", r#"{"timerel": "between", "timeAt": "2026-01-01T00:00:00Z"}"#),
        (
            "before-with-end",
            r#"{"timerel": "before", "timeAt": "2026-01-01T00:00:00Z", "endTimeAt": "2026-02-01T00:00:00Z"}"#,
        ),
        (
            "inverted",
            r#"{"timerel": "between", "timeAt": "2026-02-01T00:00:00Z", "endTimeAt": "2026-01-01T00:00:00Z"}"#,
        ),
    ];
    for (suffix, temporal_q) in temporal_queries {
        let id = format!("urn:ngsi-ld:Snapshot:{namespace}-{suffix}").parse::<SnapshotId>()?;
        let body = format!(
            r#"{{"id": "{id}", "type": "Snapshot", "snapshotTemporalQueries": [{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}], "temporalQ": {temporal_q}}}]}}"#
        );
        expect_refused(&broker, &body, "endTimeAt").await?;
        expect_absent(&broker, &id).await;
    }
    Ok(())
}
