//! Delete snapshot removes it and refuses an unknown or malformed id.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Delete Snapshot answers 204 with no body and the Snapshot is gone; deleting a Snapshot that does not exist
/// answers 404 `ResourceNotFound` and an id that is not a URI 400 `BadRequestData`
/// Clause: 5.16.5.4, 6.37.3.3
#[gtest]
#[tokio::test]
async fn n_snp_del_8100_delete_snapshot_removes_it_and_refuses_an_unknown_or_malformed_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let absent_id = format!("urn:ngsi-ld:Snapshot:{namespace}-absent").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    let gone = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await;
    expect_that!(&gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let unknown = broker.with_responses().snapshots().delete(&absent_id).send().await;
    expect_that!(&unknown, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The identifier is no URI, which no typed identifier holds, so the target is written as given.
    let malformed = request_to(&broker, Method::DELETE, "snapshots/not%20a%20URI")?;
    let answer = broker.send_unchecked(malformed).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
