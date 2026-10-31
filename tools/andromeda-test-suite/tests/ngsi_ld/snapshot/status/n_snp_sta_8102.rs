//! A snapshot resource answers its methods and refuses others.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem, problem_status},
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotFragment, SnapshotId, SnapshotPriority},
};

/// Pins: A Snapshot resource answers GET 200, PATCH 200 and DELETE 204, refuses PUT with 405, and once deleted
/// answers GET 404 `ResourceNotFound`
/// Clause: 6.37.3, 5.16.3.4, 5.16.4.4, 5.16.5.4
#[gtest]
#[tokio::test]
async fn n_snp_sta_8102_a_snapshot_resource_answers_its_methods_and_refuses_others() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&snapshot_id));
    let updated = broker
        .snapshots()
        .update(&snapshot_id, &SnapshotFragment::builder().priority(8).build()?)
        .send()
        .await?;
    expect_that!(updated.priority(), some(eq(SnapshotPriority::try_new(8)?)));
    // No typed operation replaces a Snapshot, so the PUT is sent as written.
    let replaced = broker
        .send_unchecked(exchange(
            &broker,
            Method::PUT,
            &format!("snapshots/{snapshot_id}"),
            &[("Content-Type", "application/json")],
            Some("{}"),
        )?)
        .await;
    expect_that!(&replaced, answered(StatusCode::METHOD_NOT_ALLOWED));
    broker.snapshots().delete(&snapshot_id).send().await?;
    let gone = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await;
    expect_that!(&gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    expect_that!(problem_status(&gone), some(eq(404)));
    Ok(())
}
