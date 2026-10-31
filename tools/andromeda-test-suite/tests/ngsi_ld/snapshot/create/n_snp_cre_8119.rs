//! Entity queries carry no temporal q and temporal queries carry one.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::SnapshotId,
};

/// Posts `body` as written to Create Snapshot and expects `BadRequestData`.
async fn expect_refused(broker: &BrokerClient, body: &str) -> Result<()> {
    // The bodies break the Snapshot data type, which the typed builders refuse, so each is written as text.
    let request = json_request_to(broker, Method::POST, "snapshots", body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
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
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
}

/// Pins: Create Snapshot answers 400 `BadRequestData` for a `snapshotQueries` element carrying `temporalQ` and for
/// a `snapshotTemporalQueries` element without one, and answers 201 with an empty body for an empty
/// `snapshotQueries`
/// Clause: 5.16.1.4, Table 5.2.41-1
#[gtest]
#[tokio::test]
async fn n_snp_cre_8119_entity_queries_carry_no_temporal_q_and_temporal_queries_carry_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let query = format!(r#"{{"type": "Query", "entities": [{{"type": "{vehicle_type}"}}]}}"#);
    let id = snapshot_id.to_string();
    expect_refused(
        &broker,
        &format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "{vehicle_type}"}}], "temporalQ": {{"timerel": "after", "timeAt": "2025-01-01T00:00:00Z"}}}}]}}"#),
    )
    .await?;
    expect_refused(
        &broker,
        &format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotTemporalQueries": [{query}]}}"#),
    )
    .await?;
    expect_absent(&broker, &snapshot_id).await;
    let empty_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let request = json_request_to(
        &broker,
        Method::POST,
        "snapshots",
        &format!(r#"{{"id": "{empty_id}", "type": "Snapshot", "snapshotQueries": []}}"#),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    broker.snapshots().delete(&empty_id).send().await?;
    Ok(())
}
