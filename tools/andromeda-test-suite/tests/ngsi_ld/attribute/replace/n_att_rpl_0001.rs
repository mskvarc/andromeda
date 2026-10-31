//! The relationship takes the new object.

use andromeda_test_suite::{attribute_matcher::relationship, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property, Relationship, Value},
};

/// Pins: Replace Attribute of a Relationship answers 204 and the Relationship points at the new object
/// Clause: 5.6.19.4, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0001_the_relationship_takes_the_new_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let off_street_parking_downtown2_id = namespace.urn("OffStreetParking", "Downtown2")?;
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
    let attribute = Attribute::from(Relationship::builder(off_street_parking_downtown2_id.clone()).build()?);
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
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("isParked", relationship().object(off_street_parking_downtown2_id.clone()))
    );
    Ok(())
}
