//! A snapshot under a scoped or unreachable context is refused and not created.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    unreachable_context::unreachable_context,
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

/// Pins: Create Snapshot under a user @context defining a term with a scoped @context answers 400 `BadRequestData`,
/// and under a Link to an @context that cannot be loaded answers 504 `LdContextNotAvailable`, also for a body that
/// is no Snapshot; no Snapshot is created
/// Clause: 5.5.6, 5.5.7, 6.3.5, 5.16.1.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8104_a_snapshot_under_a_scoped_or_unreachable_context_is_refused_and_not_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable_linked = broker.with_context(unreachable_context("unreachable.jsonld")?)?;
    let snapshot_1_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let snapshot_3_id = format!("urn:ngsi-ld:Snapshot:{namespace}-3").parse::<SnapshotId>()?;
    // The scoped context and the bodies that are no Snapshot are refused by the model, so each is written as text.
    let scoped = format!(
        r#"{{"id": "{snapshot_1_id}", "type": "Snapshot", "snapshotQueries": [], "@context": [{{"speed": {{"@id": "https://example.org/{namespace}/speed", "@context": {{}}}}}}, "{}"]}}"#,
        config.core_context()
    );
    let request = content_type(json_request_to(&broker, Method::POST, "snapshots", &scoped)?, &APPLICATION_LD_JSON)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    expect_absent(&broker, &snapshot_1_id).await;
    let valid = format!(
        r#"{{"id": "{snapshot_2_id}", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}]}}]}}"#
    );
    let request_2 = json_request_to(&unreachable_linked, Method::POST, "snapshots", &valid)?;
    let answer_2 = unreachable_linked.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    expect_absent(&broker, &snapshot_2_id).await;
    let malformed = format!(r#"{{"id": "{snapshot_3_id}", "type": "Snapshot", "snapshotQueries": 5}}"#);
    let request_3 = json_request_to(&unreachable_linked, Method::POST, "snapshots", &malformed)?;
    let answer_3 = unreachable_linked.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    expect_absent(&broker, &snapshot_3_id).await;
    Ok(())
}
