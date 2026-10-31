//! Retrieve with the wildcard answers the none entry when no language is tagged.

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

/// Pins: Retrieve Entity with lang=* answers the @none entry of a `languageMap` that holds no tagged language, with
/// a lang sub-property @none
/// Clause: 4.15, 5.7.1.5
/// Recorded: clause 4.15 has lang=* return a string in any supported language and clause 5.7.1.5 leaves the choice
/// to the implementation; the @none key is the only entry here and Andromeda names it @none in the lang
/// sub-property
#[gtest]
#[tokio::test]
async fn o_qry_lng_0005_retrieve_with_the_wildcard_answers_the_none_entry_when_no_language_is_tagged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property("street", LanguageProperty::builder(LanguageMap::try_from_pairs([("@none", "Default Street")])?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .lang("*".parse::<LanguageFilter>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "street"), some(eq(3)));
    expect_that!(entity, has_attribute("street", property().value("Default Street").lang("@none")));
    Ok(())
}
