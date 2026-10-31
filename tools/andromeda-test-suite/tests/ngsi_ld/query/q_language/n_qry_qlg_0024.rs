//! A sub attribute three levels down is compared.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
    query::q,
};

/// Pins: Query Entities with q on a sub-attribute three levels below the attribute answers the entity holding that
/// value at that path
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0024_a_sub_attribute_three_levels_down_is_compared() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property(
            "p",
            Property::builder(1).property(
                "accuracy",
                Property::builder(Value::Number("0.5".parse()?)).property("method", Property::builder("probe").property("version", 3)),
            ),
        )
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .property(
            "p",
            Property::builder(1).property(
                "accuracy",
                Property::builder(Value::Number("0.5".parse()?)).property("method", Property::builder("probe").property("version", 4)),
            ),
        )
        .build()?;
    broker.entities().create(&sensor_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("p").sub_attr("accuracy").sub_attr("method").sub_attr("version").eq(3))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
