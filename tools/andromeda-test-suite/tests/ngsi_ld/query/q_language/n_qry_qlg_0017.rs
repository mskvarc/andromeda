//! A sub attribute number is compared by a q ordering.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Number, Property, Value},
    query::q,
};

/// Pins: Query Entities with q comparing a sub-attribute number by > answers the entity whose sub-attribute value
/// is greater
/// Clause: 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0017_a_sub_attribute_number_is_compared_by_a_q_ordering() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.5".parse()?)))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").gt("0.4".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
