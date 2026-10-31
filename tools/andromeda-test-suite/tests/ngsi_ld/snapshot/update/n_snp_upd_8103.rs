//! A context inside an update fragment is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{problem, problem_status},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority},
};

/// Pins: Update Snapshot Status with a fragment whose `receiverInfo` element carries an @context answers 400
/// `BadRequestData` and leaves the Snapshot without `receiverInfo`
/// Clause: 5.5.7, 5.16.4.4
#[gtest]
#[tokio::test]
async fn n_snp_upd_8103_a_context_inside_an_update_fragment_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    // The fragment model has no place for a context inside a receiverInfo element, so it is sent as written.
    let refused = broker
        .send_unchecked(json_request_to(
            &broker,
            Method::PATCH,
            &format!("snapshots/{snapshot_id}"),
            r#"{
    "snapshotPriority": 9,
    "receiverInfo": [
        {
            "@context": {}
        }
    ]
}"#,
        )?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    expect_that!(problem_status(&refused), some(eq(400)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(retrieved.receiver_info(), is_empty());
    expect_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(5)?)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
