//! A vocab property array reads back with both terms.

use andromeda_test_suite::{attribute_matcher::attribute_of, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, NgsiLdMediaType, VocabProperty, VocabTerm},
};

/// Pins: an entity created with a `VocabProperty` whose vocab is an array of two terms reads back with a
/// `VocabProperty` holding both terms
/// Clause: 5.6.1, 4.5.20, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0014_a_vocab_property_array_reads_back_with_both_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .vocab_property(
            "vocabProperty",
            VocabProperty::builder(vec!["residential".parse::<VocabTerm>()?, "commercial".parse::<VocabTerm>()?]),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(
        entity,
        has_attribute(
            "vocabProperty",
            attribute_of(AttributeKind::VocabProperty)
                .list_length(2)
                .vocab_holding("residential".parse::<VocabTerm>()?)
                .vocab_holding("commercial".parse::<VocabTerm>()?)
        )
    );
    Ok(())
}
