//! A property and its sub attribute read back with their own unit codes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: a Property with one `unitCode` and a sub-attribute Property with another read back normalized with each
/// its own `unitCode`, and a Property written without one reads back without one
/// Clause: 4.5.2.2, 5.2.5, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_uni_0001_a_property_and_its_sub_attribute_read_back_with_their_own_unit_codes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property(
            "weight",
            Property::builder(5)
                .unit_code(UnitCode::Kgm)
                .property("accuracy", Property::builder(Value::Number("0.1".parse()?)).unit_code(UnitCode::Grm)),
        )
        .property("serial", "sn-1")
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "weight",
            any_attribute()
                .value(5)
                .unit_code(UnitCode::Kgm)
                .sub_attribute("accuracy", any_attribute().value(Value::Number("0.1".parse()?)).unit_code(UnitCode::Grm))
        )
    );
    expect_that!(entity, has_attribute("serial", any_attribute().value("sn-1").without(Member::UnitCode)));
    Ok(())
}
