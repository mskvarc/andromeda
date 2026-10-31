//! A sub attribute equality every entity matches pages in identifier order.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType, Number, Property, Value},
    query::q,
};

/// Pins: Query Entities with q equating a sub-attribute value all 1,500 entities of the type hold answers the first
/// hundred in identifier order with a total of 1,500 and the next page the following hundred
/// Clause: 4.9, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0026_a_sub_attribute_equality_every_entity_matches_pages_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_0001_id = namespace.urn("Sensor", "0001")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0050_id = namespace.urn("Sensor", "0050")?;
    let sensor_0100_id = namespace.urn("Sensor", "0100")?;
    let sensor_0101_id = namespace.urn("Sensor", "0101")?;
    let sensor_0150_id = namespace.urn("Sensor", "0150")?;
    let sensor_0200_id = namespace.urn("Sensor", "0200")?;
    let entities = (1..=750)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sensor", format!("{n:04}"))?)
                .entity_type(sensor_type.clone())
                .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.9".parse()?)))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let entities_2 = (751..=1500)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sensor", format!("{n:04}"))?)
                .entity_type(sensor_type.clone())
                .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.9".parse()?)))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").eq("0.9".parse::<Number>()?))
        .limit(100)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1500)));
    expect_that!(page.items(), len(eq(100)));
    expect_that!(page.items(), nth(0, has_id(&sensor_0001_id)));
    expect_that!(page.items(), nth(49, has_id(&sensor_0050_id)));
    expect_that!(page.items(), nth(99, has_id(&sensor_0100_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").eq("0.9".parse::<Number>()?))
        .limit(100)
        .offset(100)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(100)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_0101_id)));
    expect_that!(page_2.items(), nth(49, has_id(&sensor_0150_id)));
    expect_that!(page_2.items(), nth(99, has_id(&sensor_0200_id)));
    Ok(())
}
