//! A sub attribute relationship object is compared by equality.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities with q equating a sub-attribute Relationship with its object answers the entity, a
/// Relationship object being an equality target
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0022_a_sub_attribute_relationship_object_is_compared_by_equality() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let device_id = namespace.urn("Device", 1)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(20).relationship("device", device_id))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature")
            .sub_attr("device")
            .eq(format!("urn:ngsi-ld:Device:{namespace}-1").as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
