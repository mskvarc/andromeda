//! Snapshot status in no acceptable type is 406 and an unknown snapshot 404 as json.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem, problem_status},
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Retrieve Snapshot Status with Accept text/plain answers 406, and of a Snapshot that does not exist with
/// Accept application/ld+json answers 404 `ResourceNotFound` as application/json
/// Clause: 6.3.4, 6.3.2, 5.16.3.4
#[gtest]
#[tokio::test]
async fn n_snp_sta_8101_snapshot_status_in_no_acceptable_type_is_406_and_an_unknown_snapshot_404_as_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let absent_id = format!("urn:ngsi-ld:Snapshot:{namespace}-absent").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    // text/plain is no media type the client can state, so the request is sent as written.
    let unacceptable = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &format!("snapshots/{snapshot_id}"),
            &[("Accept", "text/plain")],
            None,
        )?)
        .await;
    expect_that!(&unacceptable, answered(StatusCode::NOT_ACCEPTABLE));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .snapshots()
        .retrieve(&absent_id)
        .send()
        .await;
    expect_that!(
        &absent,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_header(CONTENT_TYPE, "application/json")
    );
    expect_that!(problem_status(&absent), some(eq(404)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
