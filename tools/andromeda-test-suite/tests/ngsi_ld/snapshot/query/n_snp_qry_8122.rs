//! Discovery under a snapshot counts what it captured and what was written to it.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    snapshot_capture::await_status,
    type_information::attribute_names,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, EntityTypeName, NgsiLdMediaType, Property, discovery::EntityTypeInfo},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};

/// Reads Entity Type Information for `entity_type`, live or under `snapshot` when given.
async fn information(broker: &BrokerClient, entity_type: &EntityTypeName, snapshot: Option<&SnapshotId>) -> Result<EntityTypeInfo> {
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    Ok(match snapshot {
        Some(id) => json.on_snapshot(id).entity_types().retrieve(entity_type).send().await?,
        None => json.entity_types().retrieve(entity_type).send().await?,
    })
}

/// Expects Entity Type Information for `entity_type` to count four Entities holding `speed` and `colour`.
async fn expect_four_cars(
    broker: &BrokerClient,
    entity_type: &EntityTypeName,
    speed: &AttributeName,
    colour: &AttributeName,
    snapshot: Option<&SnapshotId>,
) -> Result<()> {
    let information = information(broker, entity_type, snapshot).await?;
    expect_that!(information.entity_count, eq(4));
    expect_that!(information.attribute_details, len(eq(2)));
    expect_that!(attribute_names(&information), unordered_elements_are![eq(speed), eq(colour)]);
    Ok(())
}

/// Expects Attribute Information for `attribute` to count `count` instances.
async fn expect_attribute_count(broker: &BrokerClient, attribute: &AttributeName, count: u64, snapshot: Option<&SnapshotId>) -> Result<()> {
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let information = match snapshot {
        Some(id) => json.on_snapshot(id).attributes().retrieve(attribute).send().await?,
        None => json.attributes().retrieve(attribute).send().await?,
    };
    expect_that!(information.attribute_count, some(eq(count)));
    Ok(())
}

/// Pins: Entity Type and Attribute Information under a Snapshot count the four captured Entities and their eight
/// speed instances as live does; after a write under the Snapshot they count the new Entity type and its instance
/// and the deleted Attribute under the Snapshot alone, and a clone of the Snapshot counts what its source counts
/// Clause: 5.5.15, 5.7.7.4, 5.7.10.4, 5.16.2.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8122_discovery_under_a_snapshot_counts_what_it_captured_and_what_was_written_to_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let bus_type = namespace.entity_type("Bus")?;
    let speed = namespace.attribute("speed")?;
    let colour = namespace.attribute("colour")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let gps = namespace.dataset_id("Dataset", "gps")?;
    let mut cars = Vec::new();
    for number in 1..=4 {
        cars.push(
            Entity::builder()
                .id(namespace.urn("Car", number)?)
                .entity_type(car_type.clone())
                .property(speed.clone(), number)
                .property(speed.clone(), Property::builder(number + 1).dataset_id(gps.clone()))
                .property(colour.clone(), "red")
                .build()?,
        );
    }
    broker.entities().batch_create(&cars).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([car_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    expect_four_cars(&broker, &car_type, &speed, &colour, Some(&snapshot_id)).await?;
    expect_attribute_count(&broker, &speed, 8, Some(&snapshot_id)).await?;
    expect_attribute_count(&broker, &colour, 4, Some(&snapshot_id)).await?;
    expect_four_cars(&broker, &car_type, &speed, &colour, None).await?;
    expect_attribute_count(&broker, &speed, 8, None).await?;
    expect_attribute_count(&broker, &colour, 4, None).await?;
    let bus = Entity::builder()
        .id(namespace.urn("Bus", 1)?)
        .entity_type(bus_type.clone())
        .property(speed.clone(), 3)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&bus).send().await?.done()?;
    broker
        .on_snapshot(&snapshot_id)
        .entities()
        .delete_attribute(&namespace.urn("Car", 1)?, &colour)
        .send()
        .await?
        .done()?;
    expect_that!(information(&broker, &bus_type, Some(&snapshot_id)).await?.entity_count, eq(1));
    let live_buses = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_types()
        .retrieve(&bus_type)
        .send()
        .await;
    expect_that!(&live_buses, answered(StatusCode::NOT_FOUND));
    expect_four_cars(&broker, &car_type, &speed, &colour, Some(&snapshot_id)).await?;
    expect_attribute_count(&broker, &speed, 9, Some(&snapshot_id)).await?;
    expect_attribute_count(&broker, &colour, 3, Some(&snapshot_id)).await?;
    expect_four_cars(&broker, &car_type, &speed, &colour, None).await?;
    expect_attribute_count(&broker, &speed, 8, None).await?;
    expect_attribute_count(&broker, &colour, 4, None).await?;
    broker
        .snapshots()
        .clone_snapshot(&snapshot_id, &SnapshotClone::builder().id(snapshot_2_id.clone()).build()?)
        .send()
        .await?;
    await_status(&broker, &snapshot_2_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    expect_four_cars(&broker, &car_type, &speed, &colour, Some(&snapshot_2_id)).await?;
    expect_attribute_count(&broker, &speed, 9, Some(&snapshot_2_id)).await?;
    expect_attribute_count(&broker, &colour, 3, Some(&snapshot_2_id)).await?;
    expect_that!(information(&broker, &bus_type, Some(&snapshot_2_id)).await?.entity_count, eq(1));
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
