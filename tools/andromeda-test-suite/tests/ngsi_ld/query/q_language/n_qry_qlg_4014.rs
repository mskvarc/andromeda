//! Unequal with a language and a value list matches an entity without that language and not one without the attribute.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: q name`[en]`!="Door","Gate" answers the entity whose en string is in neither listed value, the entity that
/// holds only a fr entry and the entity whose only entry is @none, and not the entity whose en string is listed nor
/// the entity that holds no name at all
/// Clause: 4.9
/// Recorded: clause 4.9 says a matching entity for the unequal operator "shall contain the target element and meet
/// any of" the listed conditions, one of which is that no matching value is found as the value of the specified
/// language key of a `languageMap`; an entity without the name attribute does not contain the target element
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4014_unequal_with_a_language_and_a_value_list_matches_an_entity_without_that_language_and_not_one_without_the_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_type = namespace.entity_type("Building")?;
    let building_gate_id = namespace.urn("Building", "gate")?;
    let building_wall_id = namespace.urn("Building", "wall")?;
    let building_porte_id = namespace.urn("Building", "porte")?;
    let building_default_id = namespace.urn("Building", "default")?;
    let building_nameless_id = namespace.urn("Building", "nameless")?;
    let entities = [
        Entity::builder()
            .id(building_door_id)
            .entity_type(building_type.clone())
            .language_property(
                "name",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Zimmer")])?),
            )
            .build()?,
        Entity::builder()
            .id(building_gate_id)
            .entity_type(building_type.clone())
            .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Gate")])?))
            .build()?,
        Entity::builder()
            .id(building_wall_id.clone())
            .entity_type(building_type.clone())
            .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Wall")])?))
            .build()?,
        Entity::builder()
            .id(building_porte_id.clone())
            .entity_type(building_type.clone())
            .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Porte")])?))
            .build()?,
        Entity::builder()
            .id(building_default_id.clone())
            .entity_type(building_type.clone())
            .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("@none", "Default")])?))
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
        .q(q::attr("name").member("en").ne(q::ValueList::try_from(vec![
            q::Value::from(q::ComparableValue::from("Door")),
            q::Value::from(q::ComparableValue::from("Gate")),
        ])?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(has_id(&building_wall_id)));
    expect_that!(page.items(), contains(has_id(&building_porte_id)));
    expect_that!(page.items(), contains(has_id(&building_default_id)));
    Ok(())
}
