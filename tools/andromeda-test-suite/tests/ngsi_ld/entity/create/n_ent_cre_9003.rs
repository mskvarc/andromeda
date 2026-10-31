//! A concise lone value member is the property value.

use andromeda_test_suite::{
    attribute_matcher::{geo_property, property},
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    problem::answered,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Create Entity with an attribute written as {"value": 42} stores a Property of value 42, and one written as
/// {"value": <a `GeoJSON` Point>} a `GeoProperty` of that geometry, each read back with only its type and value
/// Clause: 4.5.2.3, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9003_a_concise_lone_value_member_is_the_property_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    let body = format!(
        r#"{{"id": "urn:ngsi-ld:Device:{namespace}-1", "type": "Device_{namespace}", "count": {{"value": 42}}, "spot": {{"value": {{"type": "Point", "coordinates": [1.0, 2.0]}}}}}}"#
    );
    // The model writes a lone concise value with its type, which is not the spelling under test, so the body is sent as written.
    let request = broker.unchecked(&CreateEntity::builder().entity(&device).build())?.body(Bytes::from(body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "count"), some(eq(2)));
    expect_that!(entity, has_attribute("count", property().value(42)));
    expect_that!(attribute_members(entity, "spot"), some(eq(2)));
    expect_that!(
        entity,
        has_attribute(
            "spot",
            geo_property()
                .value_member(["type"], "Point")
                .value_member(["coordinates", "0"], Value::Number("1.0".parse()?))
                .value_member(["coordinates", "1"], Value::Number("2.0".parse()?))
        )
    );
    Ok(())
}
