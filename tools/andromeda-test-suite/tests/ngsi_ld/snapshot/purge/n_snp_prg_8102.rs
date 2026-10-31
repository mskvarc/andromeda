//! Purge snapshots takes every query the grammar allows on snapshot members.

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

/// Pins: Purge Snapshots deletes the Snapshots selected by a q on `snapshotLifetime`, by a greater-than on
/// `snapshotPriority` and by a conjunction beside a disjunction (the conjunction taking precedence), and answers
/// 404 `ResourceNotFound` for a well-formed q selecting none; the unselected Snapshot remains
/// Clause: 5.16.7.4, 4.9, Table 5.2.41-1
#[gtest]
#[tokio::test]
async fn n_snp_prg_8102_purge_snapshots_takes_every_query_the_grammar_allows_on_snapshot_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_a_id = format!("urn:ngsi-ld:Snapshot:{namespace}-a").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_b_id = format!("urn:ngsi-ld:Snapshot:{namespace}-b").parse::<SnapshotId>()?;
    let snapshot_c_id = format!("urn:ngsi-ld:Snapshot:{namespace}-c").parse::<SnapshotId>()?;
    let snapshot_d_id = format!("urn:ngsi-ld:Snapshot:{namespace}-d").parse::<SnapshotId>()?;
    let snapshot_e_id = format!("urn:ngsi-ld:Snapshot:{namespace}-e").parse::<SnapshotId>()?;
    let snapshot_f_id = format!("urn:ngsi-ld:Snapshot:{namespace}-f").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-a").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .priority(9)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let snapshot_2 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-b").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .priority(10)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_2).send().await?;
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-c").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT2H".parse()?)
        .priority(7)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_3).send().await?;
    let snapshot_4 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-d").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .priority(2)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_4).send().await?;
    let snapshot_5 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-e").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .priority(3)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_5).send().await?;
    let snapshot_6 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-f").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .priority(4)
        .endpoint(sink.endpoint("purge")?)
        .build()?;
    broker.snapshots().create(&snapshot_6).send().await?;
    broker
        .snapshots()
        .purge(
            q::attr("snapshotLifetime")
                .eq("PT2H")
                .and(q::attr("endpoint").eq(sink.endpoint("purge")?.as_str())),
        )
        .send()
        .await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_c_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_a_id)
        .send()
        .await?;
    broker
        .snapshots()
        .purge(q::attr("snapshotPriority").gt(8).and(q::attr("endpoint").eq(sink.endpoint("purge")?.as_str())))
        .send()
        .await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_a_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_b_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_d_id)
        .send()
        .await?;
    broker
        .snapshots()
        .purge(
            q::attr("id")
                .eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-d").parse::<Uri>()?)?)
                .and(q::attr("snapshotPriority").eq(2))
                .or(q::attr("id").eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Snapshot:{namespace}-e").parse::<Uri>()?)?)),
        )
        .send()
        .await?;
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_d_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_e_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .snapshots()
        .purge(q::attr("snapshotPriority").eq(11).and(q::attr("endpoint").eq(sink.endpoint("purge")?.as_str())))
        .send()
        .await;
    expect_that!(&answer_6, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let snapshot_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_f_id)
        .send()
        .await?;
    expect_that!(snapshot_7.priority(), some(eq(SnapshotPriority::try_new(4)?)));
    broker.snapshots().delete(&snapshot_f_id).send().await?;
    Ok(())
}
