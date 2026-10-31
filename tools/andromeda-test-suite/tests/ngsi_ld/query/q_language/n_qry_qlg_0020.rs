//! A sub attribute boolean is compared by equality.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities with q equating a sub-attribute boolean with true answers the entity whose sub-attribute is
/// true
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0020_a_sub_attribute_boolean_is_compared_by_equality() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(20).property("calibrated", true))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(21).property("calibrated", false))
        .build()?;
    broker.entities().create(&sensor_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("calibrated").eq(true))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
