//! Existence of a sub attribute language with a language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: q door.label`[en]` matches both entities, which hold an en label, and door.label`[fr]` matches neither
/// Clause: 4.9
/// Recorded: clause 4.9 existence only needs the entity to contain the target element, and does not say whether
/// naming a language in a query term with no operator also requires the language entry; Andromeda requires the
/// entry
#[gtest]
#[tokio::test]
async fn o_qry_qlg_0015_existence_of_a_sub_attribute_language_with_a_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_front_id = namespace.urn("Building", "front")?;
    let building_type = namespace.entity_type("Building")?;
    let building_back_id = namespace.urn("Building", "back")?;
    let building_front = Entity::builder()
        .id(building_front_id.clone())
        .entity_type(building_type.clone())
        .property(
            "door",
            Property::builder(1).language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Front"), ("de", "Vorne")])?),
            ),
        )
        .build()?;
    broker.entities().create(&building_front).send().await?.done()?;
    let building_back = Entity::builder()
        .id(building_back_id.clone())
        .entity_type(building_type.clone())
        .property(
            "door",
            Property::builder(1).language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Back"), ("de", "Hinten")])?),
            ),
        )
        .build()?;
    broker.entities().create(&building_back).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("en").exists())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_front_id)));
    expect_that!(page.items(), contains(has_id(&building_back_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("fr").exists())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
