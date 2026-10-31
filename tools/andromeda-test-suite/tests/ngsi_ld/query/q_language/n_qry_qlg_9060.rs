//! A sub attribute number matches an equal operand of any scale.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Number, Property, Value},
    query::q,
};

/// Pins: Query Entities with q equal to a number on a sub-attribute selects the Entity whose sub-attribute holds
/// that number at another scale, and only it
/// Clause: 4.9, 4.6.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9060_a_sub_attribute_number_matches_an_equal_operand_of_any_scale() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property(
            "temperature",
            Property::builder(Value::Number("21.5".parse()?)).property("accuracy", Value::Number("1.00".parse()?)),
        )
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(Value::Number("21.5".parse()?)).property("accuracy", 2))
        .build()?;
    broker.entities().create(&sensor_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").eq(1))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").eq("1.0".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").eq("2.000".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_2_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").eq(3))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    Ok(())
}
