//! Every attribute write keeps a one element json array.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeKind, AttributeName, Entity, EntityFragment, JsonProperty, NgsiLdMediaType, RawJson},
};
use serde_json::json;

/// Pins: Partial Attribute Update, Append Attributes, Merge Entity and Replace Attribute each writing a `JsonProperty`
/// holding a one-element array answer 204, and each reads back as that array
/// Clause: 4.5.24.2, 5.6.3, 5.6.4, 5.6.17, 5.6.19
#[gtest]
#[tokio::test]
async fn n_att_pat_9008_every_attribute_write_keeps_a_one_element_json_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let json_array_probe_write_id = namespace.urn("JsonArrayProbe", "write")?;
    let json_array_probe_type = namespace.entity_type("JsonArrayProbe")?;
    let json_array_probe_write = Entity::builder()
        .id(json_array_probe_write_id.clone())
        .entity_type(json_array_probe_type)
        .json_property("tickets", JsonProperty::builder(r#"{"code": "a"}"#.parse::<RawJson>()?))
        .json_property("replaced", JsonProperty::builder(r#"{"code": "x"}"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().create(&json_array_probe_write).send().await?.done()?;
    let patch = Attribute::from(JsonProperty::builder(r#"[{"code": "b"}]"#.parse::<RawJson>()?).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&json_array_probe_write_id, &"tickets".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let fragment = EntityFragment::builder()
        .json_property("appended", JsonProperty::builder(r#"[{"code": "c"}]"#.parse::<RawJson>()?))
        .build()?;
    broker
        .entities()
        .append_attributes(&json_array_probe_write_id, &fragment)
        .send()
        .await?
        .done()?;
    let fragment_2 = EntityFragment::builder()
        .json_property("merged", JsonProperty::builder(r#"[{"code": "d"}]"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().merge(&json_array_probe_write_id, &fragment_2).send().await?.done()?;
    let attribute = Attribute::from(JsonProperty::builder(r#"[{"code": "e"}]"#.parse::<RawJson>()?).build()?);
    broker
        .entities()
        .replace_attribute(&json_array_probe_write_id, &"replaced".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&json_array_probe_write_id)
        .send()
        .await?;
    let entity = retrieved.body();
    for (name, code) in [("tickets", "b"), ("appended", "c"), ("merged", "d"), ("replaced", "e")] {
        expect_that!(
            entity,
            has_attribute(name, attribute_of(AttributeKind::JsonProperty).json(json!([{"code": code}])))
        );
        expect_that!(attribute_members(entity, name), some(eq(2)));
    }
    Ok(())
}
