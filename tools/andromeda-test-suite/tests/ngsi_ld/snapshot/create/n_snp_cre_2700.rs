//! A context nested in a snapshot query is bad request data.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    wire::content_type,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{APPLICATION_LD_JSON, ErrorType, NgsiLdMediaType},
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

/// Pins: Create Snapshot refuses a user @context written inside one of its `snapshotQueries`, below the top level
/// of the body, with 400 `BadRequestData` and stores nothing
/// Clause: 5.5.7, 5.16.1
/// Recorded: clause 5.5.7 "should" raise `BadRequestData` for a user @context nested below the top level of an
/// NGSI-LD document, and a Snapshot body carries Query objects that could hold one
#[gtest]
#[tokio::test]
async fn n_snp_cre_2700_a_context_nested_in_a_snapshot_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    // The query document carries a user `@context` the model refuses, so the body is written as text.
    let text = format!(
        r#"{{"id": "{snapshot_id}", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "Building_{namespace}"}}], "@context": {{"Building_{namespace}": "https://example.org/{namespace}/Building"}}}}], "@context": "{}"}}"#,
        config.core_context()
    );
    let request = content_type(json_request_to(&broker, Method::POST, "snapshots", &text)?, &APPLICATION_LD_JSON)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    expect_absent(&broker, &snapshot_id).await;
    Ok(())
}
