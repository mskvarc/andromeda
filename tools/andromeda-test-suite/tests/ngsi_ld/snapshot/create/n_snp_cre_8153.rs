//! A temporal query with a zero last n is bad request data.

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

/// Pins: Create Snapshot with a `snapshotTemporalQueries` entry whose `temporalQ` has `lastN` 0 answers 400
/// `BadRequestData` and stores nothing
/// Clause: Table 5.2.21-1, 5.2.41, 5.5.4, 5.16.1
#[gtest]
#[tokio::test]
async fn n_snp_cre_8153_a_temporal_query_with_a_zero_last_n_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id_1 = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let body_1 = format!(
        r#"{{"id": "{id_1}", "type": "Snapshot", "snapshotTemporalQueries": [{{"type": "Query", "entities": [{{"type": "Building_{namespace}"}}], "temporalQ": {{"timerel": "after", "timeAt": "2000-01-01T00:00:00Z", "lastN": 0}}}}]}}"#
    );
    expect_refused(&broker, &body_1, "").await?;
    expect_absent(&broker, &id_1).await;
    Ok(())
}
