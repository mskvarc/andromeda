//! A structured value member is compared.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
    query::q,
};

/// Pins: Query Entities with q on a member of a structured Property value answers the entity whose member holds the
/// operand
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0032_a_structured_value_member_is_compared() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("address", object([("city", Value::from("Madrid")), ("street", Value::from("Gran Via"))]))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .property("address", object([("city", Value::from("Avila"))]))
        .build()?;
    broker.entities().create(&sensor_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("address").member("city").eq("Madrid"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
