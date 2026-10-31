//! Negated pattern with a language matches an entity without that language on an entry of its own.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: q name`[de]`!~="^Z" and q door.label`[en]`!~="^F" answer the entity whose `LanguageProperty` holds no
/// entry for the language, a default @none entry or none beside other languages, and not the entity whose entry
/// matches the pattern nor the entity that holds no such attribute at all
/// Clause: 4.9
/// Recorded: clause 4.9 says a matching entity for `notPatternOp` shall contain the target element and the target value
/// shall not be in the L(R); the entity whose `LanguageProperty` holds no entry for the language contains the target
/// element and has no value for that key, so the target value is not in L(R), the way unequal matches it; the clause
/// does not spell it out for `notPatternOp`; an entity without the attribute does not contain the target element
#[gtest]
#[tokio::test]
async fn o_qry_qlg_4018_negated_pattern_with_a_language_matches_an_entity_without_that_language_on_an_entry_of_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_zimmer_id = namespace.urn("Building", "zimmer")?;
    let building_type = namespace.entity_type("Building")?;
    let building_dach_id = namespace.urn("Building", "dach")?;
    let building_english_id = namespace.urn("Building", "english")?;
    let building_nameless_id = namespace.urn("Building", "nameless")?;
    let entities = [
        Entity::builder()
            .id(building_zimmer_id)
            .entity_type(building_type.clone())
            .language_property(
                "name",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "Zimmer"), ("en", "Room")])?),
            )
            .property(
                "door",
                Property::builder(1).language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Front")])?)),
            )
            .build()?,
        Entity::builder()
            .id(building_dach_id.clone())
            .entity_type(building_type.clone())
            .language_property(
                "name",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "Dach"), ("en", "Roof")])?),
            )
            .property(
                "door",
                Property::builder(1).language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Back")])?)),
            )
            .build()?,
        Entity::builder()
            .id(building_english_id.clone())
            .entity_type(building_type.clone())
            .language_property(
                "name",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Zone"), ("@none", "Zone")])?),
            )
            .property(
                "door",
                Property::builder(1).language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "Vorne")])?)),
            )
            .build()?,
        Entity::builder()
            .id(building_nameless_id)
            .entity_type(building_type.clone())
            .property("height", 3)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("name").member("de").not_matches("\"^Z\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_dach_id)));
    expect_that!(page.items(), contains(has_id(&building_english_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("en").not_matches("\"^F\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_dach_id)));
    expect_that!(page_2.items(), contains(has_id(&building_english_id)));
    Ok(())
}
