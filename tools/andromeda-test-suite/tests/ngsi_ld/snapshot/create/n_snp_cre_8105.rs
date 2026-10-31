//! A context inside an entity selector of a snapshot query is bad request data.

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

/// Pins: Create Snapshot whose `snapshotQueries` entity selector carries an @context answers 400 `BadRequestData`
/// and creates no Snapshot
/// Clause: 5.5.7, 5.16.1.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8105_a_context_inside_an_entity_selector_of_a_snapshot_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    // The entity selector carries an `@context` the model refuses, so the body is written as text.
    let text = format!(
        r#"{{"id": "{snapshot_id}", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}", "@context": {{}}}}]}}]}}"#
    );
    let request = json_request_to(&broker, Method::POST, "snapshots", &text)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    expect_absent(&broker, &snapshot_id).await;
    Ok(())
}
