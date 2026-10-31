//! A sub attribute string matches an anchored pattern.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities with q matching a sub-attribute String against an anchored pattern answers the entity whose
/// String matches it whole
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0023_a_sub_attribute_string_matches_an_anchored_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(20).property("source", "sensor-43"))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(21).property("source", "sensor-430"))
        .build()?;
    broker.entities().create(&sensor_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("source").matches("\"^sensor-4[0-9]$\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
