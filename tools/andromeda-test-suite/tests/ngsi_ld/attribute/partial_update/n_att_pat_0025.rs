//! A vocab property takes the new vocab and keeps its sub attribute.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeKind, AttributeName, Entity, Lossless, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: Partial Attribute Update of a `VocabProperty` with a vocab member answers 204, the Attribute stays a
/// `VocabProperty` with the new vocab and keeps its sub-Property
/// Clause: 5.6.4.4, 5.5.8, 4.5.20
#[gtest]
#[tokio::test]
async fn n_att_pat_0025_a_vocab_property_takes_the_new_vocab_and_keeps_its_sub_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .vocab_property(
            "vocabProperty",
            VocabProperty::builder("residential".parse::<VocabTerm>()?).property("accuracy", Value::Number("0.95".parse()?)),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let patch = Attribute::from(VocabProperty::builder("commercial".parse::<VocabTerm>()?).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&building_id, &"vocabProperty".parse::<AttributeName>()?, patch)
        .form(Lossless::Concise)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "vocabProperty",
            attribute_of(AttributeKind::VocabProperty)
                .vocab("commercial".parse::<VocabTerm>()?)
                .sub_attribute("accuracy", any_attribute().value(Value::Number("0.95".parse()?)))
        )
    );
    Ok(())
}
