//! Unequal with a language matches an entity without that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: q name`[en]`!="Door" and name`[en]`!="C".."E" answer the entity whose en string differs and the entity
/// that holds no en entry, and not the entity whose en string is the value or in the range
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0016_unequal_with_a_language_matches_an_entity_without_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_type = namespace.entity_type("Building")?;
    let building_gate_id = namespace.urn("Building", "gate")?;
    let building_porte_id = namespace.urn("Building", "porte")?;
    let building_door = Entity::builder()
        .id(building_door_id)
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
        .id(building_porte_id.clone())
        .entity_type(building_type.clone())
        .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Porte")])?))
        .build()?;
    broker.entities().create(&building_porte).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("name").member("en").ne("Door"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_gate_id)));
    expect_that!(page.items(), contains(has_id(&building_porte_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("name").member("en").ne(q::Range::new("C", "E")))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_gate_id)));
    expect_that!(page_2.items(), contains(has_id(&building_porte_id)));
    Ok(())
}
