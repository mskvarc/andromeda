//! Retrieve with an unmatched language answers a property of one existing entry.

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

/// Pins: Retrieve Entity with a lang no entry matches answers a Property, not the raw `languageMap`, whose value is
/// the string of one existing entry and whose lang names that entry's key
/// Clause: 4.15, 5.7.1.5
/// Recorded: clause 5.7.1.5: if no match can be made for a `LanguageProperty` a single language shall be chosen, up
/// to the implementation (and clause 4.15: the broker shall default to any supported language); the test accepts
/// any entry and checks that the value and the lang come from the same one
#[gtest]
#[tokio::test]
async fn o_qry_lng_0004_retrieve_with_an_unmatched_language_answers_a_property_of_one_existing_entry() -> Result<()> {
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
        .lang("ja".parse::<LanguageFilter>()?)
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
