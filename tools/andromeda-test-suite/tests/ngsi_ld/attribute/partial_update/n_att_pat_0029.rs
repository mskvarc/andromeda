//! The instance with the fragment dataset id takes the new value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, Lossless, NgsiLdMediaType, Property, Value},
};

/// Pins: Partial Attribute Update with a Fragment whose `datasetId` names one of two instances answers 204, changes
/// that instance's value, keeps its other members and leaves the other instance alone
/// Clause: 5.6.4.4, 5.5.8, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_att_pat_0029_the_instance_with_the_fragment_dataset_id_takes_the_new_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speedometer_dataset = namespace.dataset_id("Property", "speedometer")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?))
                .unit_code(UnitCode::Kmh)
                .dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let patch = Attribute::from(Property::builder(65).dataset_id(namespace.dataset_id("Property", "speedometer")?).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, patch)
        .form(Lossless::Concise)
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
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset).value(65).unit_code(UnitCode::Kmh))
    );
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(55)));
    Ok(())
}
