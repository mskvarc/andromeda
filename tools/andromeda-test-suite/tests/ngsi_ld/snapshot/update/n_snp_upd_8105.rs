//! An update with a receiver info entry that is not an http header is bad request data and keeps the stored one.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, KeyValuePair, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotFragment, SnapshotId},
};

/// Pins: Update Snapshot with a `receiverInfo` key with a space or a value with a line break beside a stored http
/// endpoint answers 400 `BadRequestData` and keeps the stored `receiverInfo`
/// Clause: 5.16.4, 5.16.6, 5.2.22
/// Recorded: the clauses require `receiverInfo` in the way required for the binding protocol; reading that as the
/// RFC 7230 header rule for an http endpoint is an inference
#[gtest]
#[tokio::test]
async fn n_snp_upd_8105_an_update_with_a_receiver_info_entry_that_is_not_an_http_header_is_bad_request_data_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint("s1")?)
        .receiver_info([KeyValuePair::new("Authorization", "Bearer abc")])
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    for entry in [KeyValuePair::new("Bad Name", "x"), KeyValuePair::new("X-A", "a\nb")] {
        let fragment = SnapshotFragment::builder().receiver_info([entry]).build()?;
        let refused = broker.with_responses().snapshots().update(&snapshot_id, &fragment).send().await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        expect_that!(retrieved.receiver_info(), len(eq(1)));
        expect_that!(retrieved.receiver_info().first().map(KeyValuePair::key), some(eq("Authorization")));
        expect_that!(retrieved.receiver_info().first().map(KeyValuePair::value), some(eq("Bearer abc")));
    }
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
