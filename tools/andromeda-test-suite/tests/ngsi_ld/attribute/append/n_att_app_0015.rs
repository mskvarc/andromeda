//! A json property reads back with its json object.

use andromeda_test_suite::{attribute_matcher::attribute_of, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, EntityFragment, JsonProperty, NgsiLdMediaType, RawJson},
};
use serde_json::json;

/// Pins: Append Attributes with a `JsonProperty` answers 204 and the Entity holds it with its json object unchanged
/// Clause: 5.6.3.4, 4.5.24
#[gtest]
#[tokio::test]
async fn n_att_app_0015_a_json_property_reads_back_with_its_json_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .json_property("jsonProperty", JsonProperty::builder(r#"{"key1": "value1", "key2": 42}"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "jsonProperty",
            attribute_of(AttributeKind::JsonProperty).json(json!({"key1": "value1", "key2": 42}))
        )
    );
    Ok(())
}
