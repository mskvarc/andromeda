//! No overwrite appends an instance with a new dataset id.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Relationship, Value},
};

/// Pins: Append Attributes with `options=noOverwrite` and an instance whose `datasetId` the Attribute does not have
/// answers 204 and adds the instance
/// Clause: 5.6.3.4, 6.6.3.1
#[gtest]
#[tokio::test]
async fn n_att_app_0009_no_overwrite_appends_an_instance_with_a_new_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let person_bob_id = namespace.urn("Person", "Bob")?;
    let newspeedometer_dataset = namespace.dataset_id("Property", "newspeedometer")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
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
            Relationship::builder(off_street_parking_downtown1_id)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", person_bob_id),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(60).dataset_id(namespace.dataset_id("Property", "newspeedometer")?))
        .build()?;
    broker
        .entities()
        .append_attributes(&vehicle_id, &fragment)
        .overwrite(Overwrite::Keep)
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
    expect_that!(entity, has_instances("speed", 3));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(newspeedometer_dataset).value(60)));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(55)));
    Ok(())
}
