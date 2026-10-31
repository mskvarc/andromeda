//! Purge snapshots refuses a parameter beside q.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Uri},
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Purge Snapshots with a valid q and limit=3 or local=true beside it answers 400 `InvalidRequest` and the
/// Snapshot remains, while the same q alone answers 204
/// Clause: 6.3.20, Table 6.36.3.2-1, 5.16.7.4
#[gtest]
#[tokio::test]
async fn n_snp_prg_9001_purge_snapshots_refuses_a_parameter_beside_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    // The client writes Purge Snapshots with its `q` alone, so the request is sent as written.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("snapshots?q=id%3D%3Durn%3Angsi-ld%3ASnapshot%3A{namespace}-1&limit=3"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_header(HeaderName::from_static("content-type"), "application/json")
    );
    // The client writes Purge Snapshots with its `q` alone, so the request is sent as written.
    let request_2 = request_to(
        &broker,
        Method::DELETE,
        &format!("snapshots?q=id%3D%3Durn%3Angsi-ld%3ASnapshot%3A{namespace}-1&local=true"),
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    broker
        .snapshots()
        .purge(q::attr("id").eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<Uri>()?)?))
        .send()
        .await?;
    Ok(())
}
