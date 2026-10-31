//! Create and delete entity under a snapshot change the snapshot alone.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdSnapshot},
    model::{Entity, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Create Entity and Delete Entity with NGSILD-Snapshot naming a Snapshot answer 201 and 204 with the same
/// header; the Entity reads under the Snapshot in between, never reads live, and is gone from the Snapshot after
/// the delete
/// Clause: 5.5.15, 6.3.22, 5.6.1.4, 5.6.6.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8100_create_and_delete_entity_under_a_snapshot_change_the_snapshot_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    let received = broker.with_responses().on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(10)));
    let received_2 = broker.with_responses().on_snapshot(&snapshot_id).entities().delete(&vehicle_id).send().await?;
    received_2.outcome.done()?;
    expect_that!(
        received_2.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
