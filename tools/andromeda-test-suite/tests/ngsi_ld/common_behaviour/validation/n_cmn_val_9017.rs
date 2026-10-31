//! A type member inside a property value is data.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Create Entity with a Property whose value is {"type": "Feature", "x": 1} answers 201, and the Property
/// reads back with that value
/// Clause: 4.5.2.2, 5.2.5, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9017_a_type_member_inside_a_property_value_is_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("shape", object([("type", Value::from("Feature")), ("x", Value::from(1))]))
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
        has_attribute("shape", property().value_member(["type"], "Feature").value_member(["x"], 1))
    );
    Ok(())
}
