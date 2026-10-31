//! An instance with a new dataset id is appended.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Value},
};

/// Pins: Update Attributes with an instance whose `datasetId` the Attribute does not have answers 204 and appends
/// the instance
/// Clause: 5.6.2.4, 6.6.3.2
#[gtest]
#[tokio::test]
async fn n_att_upd_0004_an_instance_with_a_new_dataset_id_is_appended() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let newspeedometer_dataset = namespace.dataset_id("Property", "newspeedometer")?;
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
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(100).dataset_id(namespace.dataset_id("Property", "newspeedometer")?))
        .build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 3));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(newspeedometer_dataset).value(100)));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(55)));
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset).value(Value::Number("54.5".parse()?)))
    );
    Ok(())
}
