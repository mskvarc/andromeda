//! Unequal with a language on a sub attribute matches an entity without that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: q door.label`[en]`!="Front" answers the entity whose sub-attribute label holds a differing en string and
/// the entity whose label holds no en entry, and not the entity whose en string is Front nor the entity whose door
/// has no label
/// Clause: 4.9
/// Recorded: clause 4.9 says a matching entity for the unequal operator "shall contain the target element and meet
/// any of" the listed conditions, one of which is that no matching value is found as the value of the specified
/// language key of a `languageMap`; the target element is the sub-attribute label, which an entity whose door has
/// none does not contain
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4016_unequal_with_a_language_on_a_sub_attribute_matches_an_entity_without_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_front_id = namespace.urn("Building", "front")?;
    let building_type = namespace.entity_type("Building")?;
    let building_back_id = namespace.urn("Building", "back")?;
    let building_german_id = namespace.urn("Building", "german")?;
    let building_plain_id = namespace.urn("Building", "plain")?;
    let entities = [
        Entity::builder()
            .id(building_front_id)
            .entity_type(building_type.clone())
            .property(
                "door",
                Property::builder(1).language_property(
                    "label",
                    LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Front"), ("de", "Vorne")])?),
                ),
            )
            .build()?,
        Entity::builder()
            .id(building_back_id.clone())
            .entity_type(building_type.clone())
            .property(
                "door",
                Property::builder(1).language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Back")])?)),
            )
            .build()?,
        Entity::builder()
            .id(building_german_id.clone())
            .entity_type(building_type.clone())
            .property(
                "door",
                Property::builder(1).language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "Hinten")])?)),
            )
            .build()?,
        Entity::builder()
            .id(building_plain_id)
            .entity_type(building_type.clone())
            .property("door", 1)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("en").ne("Front"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_back_id)));
    expect_that!(page.items(), contains(has_id(&building_german_id)));
    Ok(())
}
