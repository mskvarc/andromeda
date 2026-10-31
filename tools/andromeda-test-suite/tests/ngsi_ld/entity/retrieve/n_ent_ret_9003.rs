//! A concise property holding a geometry beside metadata keeps its type.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
    value::from_json,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, Lossless, NgsiLdMediaType, Property},
};
use serde_json::json;

/// Pins: Retrieve Entity with options=concise of a Property whose value is a `GeoJSON` Point and which carries
/// `observedAt` answers it with its type, value and `observedAt`, and that concise attribute written back reads
/// back normalized as a Property
/// Clause: 4.5.2.3, 5.2.5, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9003_a_concise_property_holding_a_geometry_beside_metadata_keeps_its_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_back_id = namespace.urn("Device", "back")?;
    let pin = from_json(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).or_fail()?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("pin", Property::builder(pin.clone()).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // A concise attribute without its `type` member would read as a GeoProperty, so a Property read back holding the
    // Point proves the member was written.
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .form(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("pin", property().value(pin.clone()).observed_at("2025-01-01T00:00:00Z".parse()?))
    );
    expect_that!(attribute_members(entity, "pin"), some(eq(3)));
    let device_back = Entity::builder()
        .id(device_back_id.clone())
        .entity_type(device_type)
        .property("pin", Property::builder(pin.clone()).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&device_back).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_back_id)
        .send()
        .await?;
    expect_that!(retrieved_2.body(), has_attribute("pin", property().value(pin)));
    Ok(())
}
