//! A vocab property is removed.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, NgsiLdMediaType, VocabProperty, VocabTerm},
};

/// Pins: Delete Attribute addressed to the `vocabProperty` `VocabProperty` answers 204 and the Entity no longer has
/// it, keeping its other Attributes
/// Clause: 5.6.5.4, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_0013_a_vocab_property_is_removed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .vocab_property("vocabProperty", VocabProperty::builder("residential".parse::<VocabTerm>()?))
        .property("name", "Eiffel Tower")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_id, &"vocabProperty".parse::<AttributeName>()?)
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
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, lacks_attribute("vocabProperty"));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
