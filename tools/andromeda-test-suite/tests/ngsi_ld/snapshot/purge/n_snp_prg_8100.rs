//! Purge snapshots deletes the snapshots its query selects.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Uri},
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority},
};

/// Pins: Purge Snapshots answers 204 for a query on id, on endpoint, on type and `snapshotPriority` together and on
/// two ids of which one exists, deleting each selected Snapshot; queries selecting none answer 404
/// `ResourceNotFound` and the unselected Snapshot remains
/// Clause: 5.16.7.4, 6.36.3.2, 4.9
#[gtest]
#[tokio::test]
async fn n_snp_prg_8100_purge_snapshots_deletes_the_snapshots_its_query_selects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let snapshot_3_id = format!("urn:ngsi-ld:Snapshot:{namespace}-3").parse::<SnapshotId>()?;
    let snapshot_4_id = format!("urn:ngsi-ld:Snapshot:{namespace}-4").parse::<SnapshotId>()?;
    let snapshot_5_id = format!("urn:ngsi-ld:Snapshot:{namespace}-5").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(1)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let snapshot_2 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(2)
        .endpoint(sink.endpoint("target")?)
        .build()?;
    broker.snapshots().create(&snapshot_2).send().await?;
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-3").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(3)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_3).send().await?;
    let snapshot_4 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-4").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(4)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_4).send().await?;
    let snapshot_5 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-5").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(5)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_5).send().await?;
    broker
        .snapshots()
        .purge(q::attr("id").eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<Uri>()?)?))
        .send()
        .await?;
    broker
        .snapshots()
        .purge(q::attr("endpoint").eq(sink.endpoint("target")?.as_str()))
        .send()
        .await?;
    broker
        .snapshots()
        .purge(
            q::attr("type")
                .eq("Snapshot")
                .and(q::attr("snapshotPriority").eq(3))
                .and(q::attr("endpoint").eq(sink.endpoint("purge")?.as_str())),
        )
        .send()
        .await?;
    broker
        .snapshots()
        .purge(
            q::attr("id")
                .eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-4").parse::<Uri>()?)?)
                .or(q::attr("id").eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-missing").parse::<Uri>()?)?)),
        )
        .send()
        .await?;
    let answer = broker.with_responses().snapshots().purge(q::attr("type").ne("Snapshot")).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .snapshots()
        .purge(q::attr("snapshotPriority").ne(5).and(q::attr("endpoint").eq(sink.endpoint("purge")?.as_str())))
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_2_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_3_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_4_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let snapshot_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_5_id)
        .send()
        .await?;
    expect_that!(snapshot_6.priority(), some(eq(SnapshotPriority::try_new(5)?)));
    broker.snapshots().delete(&snapshot_5_id).send().await?;
    Ok(())
}
