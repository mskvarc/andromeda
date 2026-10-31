//! A replace attribute after the history was deleted keeps the entity type on the evolution.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Property, Value, VocabProperty, VocabTerm},
};

/// Pins: Replace Attribute temperature with 5.5 on an Entity whose Temporal Evolution was deleted first answers 204
/// and the Entity holds 5.5, and after an append the Temporal Evolution carries the Entity type
/// Clause: 4.5.6, 5.6.16.4, 5.6.19.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_att_rpl_9004_a_replace_attribute_after_the_history_was_deleted_keeps_the_entity_type_on_the_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_replace_id = namespace.urn("Building", "replace")?;
    let building_type = namespace.entity_type("Building")?;
    let building_replace = Entity::builder()
        .id(building_replace_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_replace).send().await?.done()?;
    broker.temporal().delete(&building_replace_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_replace_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let attribute = Attribute::from(Property::builder(Value::Number("5.5".parse()?)).build()?);
    broker
        .entities()
        .replace_attribute(&building_replace_id, &"temperature".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_replace_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(Value::Number("5.5".parse()?))));
    let fragment = EntityFragment::builder().property("humidity", 44).build()?;
    broker.entities().append_attributes(&building_replace_id, &fragment).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_replace_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution.entity_type, eq(&OneOrMany::one(building_type.clone())));
    Ok(())
}
