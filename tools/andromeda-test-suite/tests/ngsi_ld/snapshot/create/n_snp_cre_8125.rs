//! The snapshot context follows the media type of the body.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    config::SuiteConfig,
    core_context::core_context_link,
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

/// Pins: Create Snapshot answers 201 for an application/json body with a Link @context and for an
/// application/ld+json body carrying its @context, and 400 `BadRequestData` for an application/json body carrying
/// an @context, an application/ld+json body without one and an application/ld+json body with a Link header
/// Clause: 6.3.5, 5.16.1.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8125_the_snapshot_context_follows_the_media_type_of_the_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let ids = (1..=5)
        .map(|n| format!("urn:ngsi-ld:Snapshot:{namespace}-{n}").parse::<SnapshotId>())
        .collect::<Result<Vec<_>, _>>()?;
    let plain = |id: &SnapshotId| format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": []}}"#);
    let with_context = |id: &SnapshotId| format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": [], "@context": "{core}"}}"#);
    // Each body is a Snapshot without queries or with a body `@context`, which the typed builders do not write, so it is written as text.
    let request = json_request_to(&broker, Method::POST, "snapshots", &plain(&ids[0]))?.typed_header(core_context_link()?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let request_2 = content_type(
        json_request_to(&broker, Method::POST, "snapshots", &with_context(&ids[1]))?,
        &APPLICATION_LD_JSON,
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let request_3 = json_request_to(&broker, Method::POST, "snapshots", &with_context(&ids[2]))?;
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let request_4 = content_type(json_request_to(&broker, Method::POST, "snapshots", &plain(&ids[3]))?, &APPLICATION_LD_JSON)?;
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let request_5 = content_type(
        json_request_to(&broker, Method::POST, "snapshots", &with_context(&ids[4]))?,
        &APPLICATION_LD_JSON,
    )?
    .typed_header(core_context_link()?);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    for id in &ids[2..] {
        expect_absent(&broker, id).await;
    }
    broker.snapshots().delete(&ids[1]).send().await?;
    broker.snapshots().delete(&ids[0]).send().await?;
    Ok(())
}
