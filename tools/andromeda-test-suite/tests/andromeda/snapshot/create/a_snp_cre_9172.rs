//! A snapshot receiver info key naming a header the broker writes itself is bad request data and stores nothing.

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
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Create Snapshot with an http endpoint whose `receiverInfo` holds a custom header and a key of `Content-Type`,
/// `Content-Length`, `Transfer-Encoding`, `Host`, `Connection`, `TE`, `Trailer`, `Upgrade` or `Keep-Alive`, in any
/// letter case, answers 400 `BadRequestData` naming `receiverInfo` and stores nothing, while the custom header alone
/// answers 201
/// Clause: 5.16.1.4, 5.16.6, 5.2.22
/// Recorded: no clause reserves a `receiverInfo` key, so this is stricter than the specification asks; the broker and
/// its HTTP client write each of these fields on every status notification, and an entry naming one would send a
/// second `Content-Type`, a length that does not match the body, two framings or a field of the connection the client
/// manages
#[gtest]
#[tokio::test]
async fn a_snp_cre_9172_a_snapshot_receiver_info_key_naming_a_header_the_broker_writes_itself_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for (n, (key, value)) in RESERVED_RECEIVER_INFO.into_iter().enumerate() {
        let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-{n}").parse::<SnapshotId>()?;
        let snapshot = Snapshot::builder()
            .id(snapshot_id.clone())
            .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
            .endpoint(sink.endpoint(n)?)
            .receiver_info([KeyValuePair::new("Authorization", "Bearer abc"), KeyValuePair::new(key, value)])
            .build()?;
        let answer = broker.with_responses().snapshots().create(&snapshot).send().await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("receiverInfo")
        );
        let retrieved = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await;
        expect_that!(&retrieved, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    let custom_id = format!("urn:ngsi-ld:Snapshot:{namespace}-custom").parse::<SnapshotId>()?;
    let custom = Snapshot::builder()
        .id(custom_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint("custom")?)
        .receiver_info([KeyValuePair::new("Authorization", "Bearer abc")])
        .build()?;
    broker.snapshots().create(&custom).send().await?;
    broker.snapshots().delete(&custom_id).send().await?;
    Ok(())
}
