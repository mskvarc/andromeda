//! An attribute with dataset instances orders by its first written instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::ordering::OrderBy,
};

/// Pins: Query Entities with `orderBy` on an attribute some entities hold in several `datasetId` instances orders
/// each entity by the instance written first, whatever `datasetId` it has
/// Clause: 4.23.1, 4.5.5
/// Recorded: 4.23.1 sorts by the target value and fixes no instance among several `datasetId` instances; Andromeda
/// orders by the instance written first
#[gtest]
#[tokio::test]
async fn o_qry_ord_0015_an_attribute_with_dataset_instances_orders_by_its_first_written_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_3_id = namespace.urn("Sensor", 3)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("speed", 30)
        .property("speed", Property::builder(5).dataset_id(namespace.dataset_id("Dataset", 1)?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id.clone())
        .entity_type(sensor_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&sensor_2).send().await?.done()?;
    let sensor_3 = Entity::builder()
        .id(sensor_3_id.clone())
        .entity_type(sensor_type.clone())
        .property("speed", Property::builder(20).dataset_id(namespace.dataset_id("Dataset", 1)?))
        .property("speed", Property::builder(40).dataset_id(namespace.dataset_id("Dataset", 2)?))
        .build()?;
    broker.entities().create(&sensor_3).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .ordering("speed".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&sensor_2_id)));
    expect_that!(page.items(), nth(1, has_id(&sensor_3_id)));
    expect_that!(page.items(), nth(2, has_id(&sensor_id)));
    Ok(())
}
