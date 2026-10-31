//! A temporal query time at with a numeric offset is bad request data.

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

/// Pins: Create Snapshot whose `snapshotTemporalQueries` element carries a `temporalQ` `timeAt` carrying a numeric
/// offset in place of Z (+01:00, or +00:00 naming UTC) answers 400 `BadRequestData`, and no Snapshot exists
/// Clause: 4.6.3, 5.2.21, 5.16.1.4, Table 5.2.41-1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8157_a_temporal_query_time_at_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    for time_at in ["2026-03-04T05:06:07+01:00", "2026-03-04T05:06:07+00:00"] {
        let body = format!(
            r#"{{"id": "{id}", "type": "Snapshot", "snapshotTemporalQueries": [{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}], "temporalQ": {{"timerel": "after", "timeAt": "{time_at}"}}}}]}}"#
        );
        expect_refused(&broker, &body, "").await?;
    }
    expect_absent(&broker, &id).await;
    Ok(())
}
