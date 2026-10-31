//! A clone takes writes of its own beside what it copied.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
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

/// Pins: A clone of a Snapshot holding captured Entities and Entities written to it takes a Create Entity, a Batch
/// Entity Creation and an Append Entity Attributes on a copied Entity, holds them beside every copied Entity, and
/// its source sees none of them
/// Clause: 5.16.2.4, 5.5.15
#[gtest]
#[tokio::test]
async fn n_snp_cln_8104_a_clone_takes_writes_of_its_own_beside_what_it_copied() -> Result<()> {
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
    let under_clone = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    under_clone
        .on_snapshot(&second_id)
        .entities()
        .create(&car("clone-write", 20)?)
        .send()
        .await?
        .done()?;
    let batch = vec![car("clone-batch-1", 21)?, car("clone-batch-2", 22)?];
    under_clone.on_snapshot(&second_id).entities().batch_create(&batch).send().await?.done()?;
    under_clone
        .on_snapshot(&second_id)
        .entities()
        .append_attributes(&namespace.urn("Car", "captured-2")?, &colour("blue")?)
        .send()
        .await?
        .done()?;
    for (suffix, speed) in [("clone-write", 20), ("clone-batch-2", 22), ("source-write", 10)] {
        let retrieved = under_clone
            .on_snapshot(&second_id)
            .entities()
            .retrieve(&namespace.urn("Car", suffix)?)
            .send()
            .await?;
        expect_that!(retrieved.body(), has_attribute("speed", property().value(speed)));
    }
    for (suffix, colour_value) in [("captured-1", "red"), ("captured-2", "blue")] {
        let retrieved = under_clone
            .on_snapshot(&second_id)
            .entities()
            .retrieve(&namespace.urn("Car", suffix)?)
            .send()
            .await?;
        expect_that!(retrieved.body(), has_attribute("colour", property().value(colour_value)));
    }
    let page = under_clone
        .on_snapshot(&second_id)
        .entities()
        .query()
        .entity_type(&car_type)
        .limit(100)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(7)));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&first_id)
        .entities()
        .retrieve(&namespace.urn("Car", "clone-write")?)
        .send()
        .await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    let source_side = under_clone
        .on_snapshot(&first_id)
        .entities()
        .retrieve(&namespace.urn("Car", "captured-2")?)
        .send()
        .await?;
    expect_that!(source_side.body(), lacks_attribute("colour"));
    broker.snapshots().delete(&second_id).send().await?;
    broker.snapshots().delete(&first_id).send().await?;
    Ok(())
}
