//! Merge and replace under a snapshot change the snapshot alone.

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
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Merge Entity, Replace Entity and Replace Attribute with NGSILD-Snapshot answer 204, each value reads under
/// the Snapshot after its write, and no Entity exists live
/// Clause: 5.5.15, 5.6.17.4, 5.6.18.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8102_merge_and_replace_under_a_snapshot_change_the_snapshot_alone() -> Result<()> {
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
        .property("speed", 1)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 2).build()?;
    broker.on_snapshot(&snapshot_id).entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(2)));
    let vehicle_replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 3)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().replace(&vehicle_replacement).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(3)));
    let attribute = Attribute::from(Property::builder(4).build()?);
    broker
        .on_snapshot(&snapshot_id)
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_attribute("speed", any_attribute().value(4)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
