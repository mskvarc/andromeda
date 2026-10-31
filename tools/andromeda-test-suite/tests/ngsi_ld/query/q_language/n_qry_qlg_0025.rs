//! A value under another attribute does not match a sub attribute path.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities with q on b.x answers no entity when only a.x holds the value, a sub-attribute being
/// addressed under its own attribute
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0025_a_value_under_another_attribute_does_not_match_a_sub_attribute_path() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id)
        .entity_type(sensor_type.clone())
        .property("a", Property::builder(1).property("x", 5))
        .property("b", 2)
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("b").sub_attr("x").eq(5))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
