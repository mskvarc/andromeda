//! A new property is added beside the existing attributes.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Relationship, Value},
};

/// Pins: Append Attributes with a Property the Entity lacks answers 204 and the Entity holds it beside its existing
/// Attributes
/// Clause: 5.6.3.4, 6.6.3.1
#[gtest]
#[tokio::test]
async fn n_att_app_0001_a_new_property_is_added_beside_the_existing_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let person_bob_id = namespace.urn("Person", "Bob")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let speedometer_dataset = namespace.dataset_id("Property", "speedometer")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone())
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", person_bob_id),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("attribute_to_be_added", 23).build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_attribute("attribute_to_be_added", property().value(23)));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(55)));
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset).value(Value::Number("54.5".parse()?)))
    );
    expect_that!(
        entity,
        has_attribute("isParked", any_attribute().object(off_street_parking_downtown1_id.clone()))
    );
    Ok(())
}
