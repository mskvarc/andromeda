//! A concise property keeps its unit code.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_json::json_absent,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, Lossless, NgsiLdMediaType, Property, Value},
};

/// Pins: a Property with a `unitCode` read back in the concise representation keeps its value and its `unitCode`,
/// only the type member being dropped
/// Clause: 4.5.2.2, 4.5.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_uni_0002_a_concise_property_keeps_its_unit_code() -> Result<()> {
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
                .unit_code(UnitCode::Cel)
                .property("accuracy", Property::builder(Value::Number("0.1".parse()?)).unit_code(UnitCode::Cel)),
        )
        .property("serial", "sn-1")
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .form(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let entity = received.outcome.body();
    expect_that!(entity, has_attribute("weight", any_attribute().value(5).unit_code(UnitCode::Cel)));
    // The model cannot tell a concise attribute writing `type` from one leaving it out, so its absence is read as
    // received.
    expect_that!(&received.body, json_absent("$.weight.type"));
    Ok(())
}
