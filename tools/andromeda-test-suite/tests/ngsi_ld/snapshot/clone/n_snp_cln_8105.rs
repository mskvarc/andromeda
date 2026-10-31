//! A clone of a written clone carries the writes and takes more.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, Input, NgsiLdMediaType, Property},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};

/// Pins: A clone of a clone that was written to after its own copy holds that write and takes a Create Entity and
/// an Append Entity Attributes on the written Entity
/// Clause: 5.16.2.4, 5.5.15
#[gtest]
#[tokio::test]
async fn n_snp_cln_8105_a_clone_of_a_written_clone_carries_the_writes_and_takes_more() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let first_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let second_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let car = |suffix: &str, speed: i32| -> Result<Entity<Input>> {
        Ok(Entity::builder()
            .id(namespace.urn("Car", suffix)?)
            .entity_type(car_type.clone())
            .property("speed", speed)
            .build()?)
    };
    let captured = vec![car("captured-1", 1)?, car("captured-2", 2)?, car("captured-3", 3)?];
    broker.entities().batch_create(&captured).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(first_id.clone())
        .query(QueryDocument::builder().entities([car_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &first_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let colour = |value: &str| -> Result<EntityFragment> { Ok(EntityFragment::builder().property("colour", Property::builder(value)).build()?) };
    broker
        .on_snapshot(&first_id)
        .entities()
        .create(&car("source-write", 10)?)
        .send()
        .await?
        .done()?;
    broker
        .on_snapshot(&first_id)
        .entities()
        .append_attributes(&namespace.urn("Car", "captured-1")?, &colour("red")?)
        .send()
        .await?
        .done()?;
    broker
        .snapshots()
        .clone_snapshot(&first_id, &SnapshotClone::builder().id(second_id.clone()).build()?)
        .send()
        .await?;
    await_status(&broker, &second_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let third_id = format!("urn:ngsi-ld:Snapshot:{namespace}-3").parse::<SnapshotId>()?;
    broker
        .on_snapshot(&second_id)
        .entities()
        .create(&car("first-write", 30)?)
        .send()
        .await?
        .done()?;
    broker
        .snapshots()
        .clone_snapshot(&second_id, &SnapshotClone::builder().id(third_id.clone()).build()?)
        .send()
        .await?;
    await_status(&broker, &third_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let reader = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first_write = reader
        .on_snapshot(&third_id)
        .entities()
        .retrieve(&namespace.urn("Car", "first-write")?)
        .send()
        .await?;
    expect_that!(first_write.body(), has_attribute("speed", property().value(30)));
    broker
        .on_snapshot(&third_id)
        .entities()
        .create(&car("second-write", 31)?)
        .send()
        .await?
        .done()?;
    broker
        .on_snapshot(&third_id)
        .entities()
        .append_attributes(&namespace.urn("Car", "first-write")?, &colour("green")?)
        .send()
        .await?
        .done()?;
    let first_write_2 = reader
        .on_snapshot(&third_id)
        .entities()
        .retrieve(&namespace.urn("Car", "first-write")?)
        .send()
        .await?;
    expect_that!(first_write_2.body(), has_attribute("colour", property().value("green")));
    let second_write = reader
        .on_snapshot(&third_id)
        .entities()
        .retrieve(&namespace.urn("Car", "second-write")?)
        .send()
        .await?;
    expect_that!(second_write.body(), has_attribute("speed", property().value(31)));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&second_id)
        .entities()
        .retrieve(&namespace.urn("Car", "second-write")?)
        .send()
        .await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&third_id).send().await?;
    broker.snapshots().delete(&second_id).send().await?;
    broker.snapshots().delete(&first_id).send().await?;
    Ok(())
}
