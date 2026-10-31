//! Purge snapshots refuses a missing or invalid query.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem, problem_status},
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    client::ClientError,
    http::headers::Accept,
    model::{DateTime, ErrorType, NgsiLdMediaType},
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};
use std::fmt::Debug;

/// Checks a Purge Snapshots answer: 400 with a `BadRequestData` problem whose `status` is 400.
fn expect_bad_request_data<V, E>(answer: &Result<V, ClientError<E>>)
where
    Result<V, ClientError<E>>: AnsweredStatus + Debug,
{
    expect_that!(answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    expect_that!(problem_status(answer), some(eq(400)));
}

/// Checks a Purge Snapshots answer as `expect_bad_request_data` does and that it is `application/json`.
fn expect_json_bad_request_data<V, E>(answer: &Result<V, ClientError<E>>)
where
    Result<V, ClientError<E>>: AnsweredStatus + Debug,
{
    expect_bad_request_data(answer);
    expect_that!(answer, answered(StatusCode::BAD_REQUEST).with_header(CONTENT_TYPE, "application/json"));
}

/// Pins: Purge Snapshots without q, with q on an output-only member or a member outside the Snapshot data type, or
/// with q breaking the query grammar answers 400 `BadRequestData` as application/json, and the Snapshot remains
/// Clause: 5.16.7.4, 6.36.3.2, 4.9
#[gtest]
#[tokio::test]
async fn n_snp_prg_8101_purge_snapshots_refuses_a_missing_or_invalid_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoint = sink.endpoint("purge")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(endpoint.clone())
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    // The typed purge has no form without a query.
    let missing = broker.send_unchecked(exchange(&broker, Method::DELETE, "snapshots", &[], None)?).await;
    expect_bad_request_data(&missing);
    let selecting = q::attr("endpoint").eq(endpoint.as_str());
    let instant = "2026-01-01T00:00:00Z".parse::<DateTime>()?;
    for member in [
        q::attr("snapshotStatus").eq("success"),
        q::attr("createdAt").eq(instant),
        q::attr("modifiedAt").eq(instant),
        q::attr("expiresAt").eq(instant),
        q::attr("lastUsedAt").eq(instant),
        q::attr("unknown").eq("value"),
    ] {
        let answer = broker.with_responses().snapshots().purge(member.and(selecting.clone())).send().await;
        expect_json_bad_request_data(&answer);
    }
    let selecting_text = format!(r#";endpoint=="{endpoint}""#);
    for member in [
        "snapshotQueries==[]".to_owned(),
        format!("id<urn:ngsi-ld:Snapshot:{namespace}-z"),
        "type>=Snapshot".to_owned(),
        "endpoint!=not-a-url".to_owned(),
        "snapshotLifetime>PT1H".to_owned(),
        "receiverInfo.key==X-Test".to_owned(),
    ] {
        // The query language has no empty list and writes a text value quoted, so these queries go out as written.
        let q_text = format!("{member}{selecting_text}");
        let refused = target("snapshots", &[("q", q_text.as_str())]);
        let answer = broker.send_unchecked(exchange(&broker, Method::DELETE, &refused, &[], None)?).await;
        expect_json_bad_request_data(&answer);
    }
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(remaining.id(), eq(&snapshot_id));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
