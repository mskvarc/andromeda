//! Retrieve with the wildcard answers a property of one existing entry.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    language_map_matcher::has_language_map,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with lang=* answers the `LanguageProperty` as a Property whose value is the string of one
/// existing entry of the `languageMap` and whose lang names that entry's key
/// Clause: 4.15, 5.7.1.5
/// Recorded: clause 4.15 has lang=* return the `LanguageProperty` as a string in any supported language and the
/// attribute carry a lang sub-property naming the language returned, and clause 5.7.1.5 leaves the single language
/// chosen up to the implementation; the test accepts any entry and checks that the value and the lang come from the
/// same one
#[gtest]
#[tokio::test]
async fn o_qry_lng_0002_retrieve_with_the_wildcard_answers_a_property_of_one_existing_entry() -> Result<()> {
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
    // The entry chosen is one of the existing entries, with its own language and string.
    expect_that!(
        entity,
        any!(
            has_attribute("street", property().value("Grand Place").lang("fr")),
            has_attribute("street", property().value("Grote Markt").lang("nl"))
        )
    );
    let unfiltered = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(unfiltered.body(), has_language_map("street", &[("fr", "Grand Place"), ("nl", "Grote Markt")]));
    Ok(())
}
