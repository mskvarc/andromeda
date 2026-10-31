//! Ordering and range with a language read the language string.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: q name`[en]`>"E", name`[en]`<="Door" and name`[en]`=="C".."E" compare the en string of a
/// `LanguageProperty` with the query value, and an entity with no en entry never matches
/// Clause: 4.9
/// Recorded: clause 4.9 makes the target value of a `LanguageProperty` with a language the string of that language
/// for every operator, but does not say how two strings are ordered or whether a range applies to a string;
/// Andromeda compares them by character code
#[gtest]
#[tokio::test]
async fn o_qry_qlg_0009_ordering_and_range_with_a_language_read_the_language_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_type = namespace.entity_type("Building")?;
    let building_gate_id = namespace.urn("Building", "gate")?;
    let building_porte_id = namespace.urn("Building", "porte")?;
    let building_door = Entity::builder()
        .id(building_door_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Zimmer")])?),
        )
        .build()?;
    broker.entities().create(&building_door).send().await?.done()?;
    let building_gate = Entity::builder()
        .id(building_gate_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Gate"), ("de", "Ausgang")])?),
        )
        .build()?;
    broker.entities().create(&building_gate).send().await?.done()?;
    let building_porte = Entity::builder()
        .id(building_porte_id)
        .entity_type(building_type.clone())
        .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Porte")])?))
        .build()?;
    broker.entities().create(&building_porte).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("name").member("en").gt("E"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_gate_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("name").member("en").le("Door"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&building_door_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("name").member("en").within("C", "E"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&building_door_id)));
    Ok(())
}
