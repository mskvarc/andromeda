//! Version 1 0 removes the unit code.

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
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Prefer, SpecVersion},
    model::{Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Retrieve Entity under Prefer ngsi-ld=1.0 removes the `unitCode` of a Property and of its sub-attribute,
/// which arrived at version 1.3, and answers 203 with the version echoed
/// Clause: 4.3.6.8, 6.3.6, 5.7.1.4, 4.5.2.2
#[gtest]
#[tokio::test]
async fn n_cmn_prf_0012_version_1_0_removes_the_unit_code() -> Result<()> {
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
                .property("accuracy", Property::builder(Value::Number("0.1".parse()?)).unit_code(UnitCode::Kgm)),
        )
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_prefer(Prefer::builder().version("1.0".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(received.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(received.outcome.preference_applied(), some(eq(&"1.0".parse::<SpecVersion>()?)));
    let entity = received.outcome.body();
    expect_that!(
        entity,
        has_attribute(
            "weight",
            any_attribute()
                .value(5)
                .without(Member::UnitCode)
                .sub_attribute("accuracy", any_attribute().value(Value::Number("0.1".parse()?)).without(Member::UnitCode))
        )
    );
    Ok(())
}
