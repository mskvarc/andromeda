//! Every shape at the limit is created and reads back.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nesting::{list_chain, object_chain, object_chain_text, sub_attribute_chain, sub_attribute_chain_matcher},
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, JsonProperty, ListProperty, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Create Entity with p nesting 32 levels, as an object chain, a list chain, a JSON literal, a sub-Attribute
/// value and a sub-Attribute chain, answers 201 each time, and Retrieve Entity reads the leaf back at its depth
/// Clause: 5.6.1.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9006_every_shape_at_the_limit_is_created_and_reads_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let leaf_json = serde_json::from_str::<serde_json::Value>(&object_chain_text(32, "leaf"))?;
    let object_chain_id = namespace.urn("NestingProbe", "object-chain")?;
    let object_chain_entity = Entity::builder()
        .id(object_chain_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property("p", object_chain(32, "leaf"))
        .build()?;
    broker.entities().create(&object_chain_entity).send().await?.done()?;
    let list_chain_id = namespace.urn("NestingProbe", "list-chain")?;
    let list_chain_entity = Entity::builder()
        .id(list_chain_id.clone())
        .entity_type(nesting_probe_type.clone())
        .list_property("p", ListProperty::builder(vec![list_chain(31)]))
        .build()?;
    broker.entities().create(&list_chain_entity).send().await?.done()?;
    let json_literal_id = namespace.urn("NestingProbe", "json-literal")?;
    let json_literal_entity = Entity::builder()
        .id(json_literal_id.clone())
        .entity_type(nesting_probe_type.clone())
        .json_property("p", JsonProperty::builder(object_chain_text(32, "leaf").parse::<RawJson>()?))
        .build()?;
    broker.entities().create(&json_literal_entity).send().await?.done()?;
    let sub_attribute_value_id = namespace.urn("NestingProbe", "sub-attribute-value")?;
    let sub_attribute_value_entity = Entity::builder()
        .id(sub_attribute_value_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property("p", Property::builder(1).property("s", object_chain(31, "leaf")))
        .build()?;
    broker.entities().create(&sub_attribute_value_entity).send().await?.done()?;
    let sub_attribute_chain_id = namespace.urn("NestingProbe", "sub-attribute-chain")?;
    let sub_attribute_chain_entity = Entity::builder()
        .id(sub_attribute_chain_id.clone())
        .entity_type(nesting_probe_type)
        .property("p", Property::builder(1).property("s", sub_attribute_chain(31)?))
        .build()?;
    broker.entities().create(&sub_attribute_chain_entity).send().await?.done()?;
    let shapes = [
        (&object_chain_id, any_attribute().value_member(["k"; 32], "leaf")),
        (&list_chain_id, any_attribute().list_length(1).list_value(0, list_chain(31))),
        (&json_literal_id, any_attribute().json(leaf_json)),
        (
            &sub_attribute_value_id,
            property().value(1).sub_attribute("s", property().value_member(["k"; 31], "leaf")),
        ),
        (&sub_attribute_chain_id, sub_attribute_chain_matcher(32)),
    ];
    for (entity_id, matcher) in shapes {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(entity_id)
            .send()
            .await?;
        expect_that!(retrieved.body(), has_attribute("p", matcher));
    }
    Ok(())
}
