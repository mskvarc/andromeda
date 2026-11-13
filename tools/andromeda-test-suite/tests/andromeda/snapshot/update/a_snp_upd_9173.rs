//! A snapshot update with a receiver info key naming a header the broker writes itself is bad request data and keeps
//! the stored one.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    reserved_receiver_info::RESERVED_RECEIVER_INFO,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, KeyValuePair, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotFragment, SnapshotId},
};

/// Pins: Update Snapshot beside a stored http endpoint with a `receiverInfo` holding a custom header and a key of
/// `Content-Type`, `Content-Length`, `Transfer-Encoding`, `Host`, `Connection`, `TE`, `Trailer`, `Upgrade` or
/// `Keep-Alive`, in any letter case, answers 400 `BadRequestData` naming `receiverInfo` and keeps the stored
/// `receiverInfo`
/// Clause: 5.16.4, 5.16.6, 5.2.22
/// Recorded: no clause reserves a `receiverInfo` key, so this is stricter than the specification asks; the broker and
/// its HTTP client write each of these fields on every status notification, and an entry naming one would send a
/// second `Content-Type`, a length that does not match the body, two framings or a field of the connection the client
/// manages
#[gtest]
#[tokio::test]
async fn a_snp_upd_9173_a_snapshot_update_with_a_receiver_info_key_naming_a_header_the_broker_writes_itself_is_bad_request_data_and_keeps_the_stored_one()
-> Result<()> {
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
    for (key, value) in RESERVED_RECEIVER_INFO {
        let fragment = SnapshotFragment::builder()
            .receiver_info([KeyValuePair::new("Authorization", "Bearer other"), KeyValuePair::new(key, value)])
            .build()?;
        let refused = broker.with_responses().snapshots().update(&snapshot_id, &fragment).send().await;
        expect_that!(
            &refused,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("receiverInfo")
        );
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        expect_that!(
            retrieved
                .receiver_info()
                .iter()
                .map(|pair| (KeyValuePair::key(pair), KeyValuePair::value(pair)))
                .collect::<Vec<_>>(),
            elements_are![eq(&("Authorization", "Bearer abc"))]
        );
    }
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
