//! The instance with the fragment dataset id is replaced.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Replace Attribute with a Fragment whose `datasetId` names one of two instances answers 204, replaces that
/// instance whole (a member the Fragment leaves out is gone) and leaves the other instance alone
/// Clause: 5.6.19.4, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0012_the_instance_with_the_fragment_dataset_id_is_replaced() -> Result<()> {
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
    let attribute = Attribute::from(Property::builder(100).dataset_id(namespace.dataset_id("Property", "speedometer")?).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute)
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
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset).value(100).without(Member::UnitCode))
    );
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(55)));
    Ok(())
}
