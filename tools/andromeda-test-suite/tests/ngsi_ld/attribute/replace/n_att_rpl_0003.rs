//! A relationship is replaced by a property.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Replace Attribute with a Property under the name of a Relationship answers 204 and the Attribute becomes
/// that Property
/// Clause: 5.6.19.4, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0003_a_relationship_is_replaced_by_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .relationship("isParked", off_street_parking_downtown1_id)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let attribute = Attribute::from(Property::builder(true).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_id, &"isParked".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("isParked", property().value(true)));
    Ok(())
}
