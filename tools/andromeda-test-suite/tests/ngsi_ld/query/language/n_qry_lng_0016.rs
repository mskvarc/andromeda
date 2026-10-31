//! Retrieve with a language converts each language property independently.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with lang=nl converts every `LanguageProperty` of the Entity, each to its own Dutch entry
/// and lang
/// Clause: 4.15, 5.7.1.5
#[gtest]
#[tokio::test]
async fn n_qry_lng_0016_retrieve_with_a_language_converts_each_language_property_independently() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .language_property(
            "description",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "A famous square"), ("nl", "Een beroemd plein")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .lang("nl".parse::<LanguageFilter>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "street"), some(eq(3)));
    expect_that!(attribute_members(entity, "description"), some(eq(3)));
    expect_that!(entity, has_attribute("street", property().value("Grote Markt").lang("nl")));
    expect_that!(entity, has_attribute("description", property().value("Een beroemd plein").lang("nl")));
    Ok(())
}
