//! A concise sibling of value is a sub attribute.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, Input, Lossless, NgsiLdMediaType, Value},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Device:{{ns}}-1",
    "type": "Device_{{ns}}",
    "temperature": {
        "value": 42,
        "accuracy": 0.5,
        "unitCode": "CEL"
    }
}"#;

/// Pins: Create Entity with an attribute written as {"value": 42, "accuracy": 0.5, "unitCode": "CEL"} stores a
/// Property of value 42 with `unitCode` CEL and an accuracy sub-Property of value 0.5, read back normalized and
/// concise
/// Clause: 4.5.2.3, 5.2.5, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9004_a_concise_sibling_of_value_is_a_sub_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device = Entity::<Input>::parse(&BODY.replace("{{ns}}", namespace.token()), Lossless::Concise)?;
    broker.entities().create(&device).form(Lossless::Concise).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "temperature",
            property()
                .value(42)
                .unit_code(UnitCode::Cel)
                .sub_attribute("accuracy", property().value(Value::Number("0.5".parse()?)))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .form(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute(
            "temperature",
            any_attribute()
                .value(42)
                .unit_code(UnitCode::Cel)
                .sub_attribute("accuracy", any_attribute().value(Value::Number("0.5".parse()?)))
        )
    );
    Ok(())
}
