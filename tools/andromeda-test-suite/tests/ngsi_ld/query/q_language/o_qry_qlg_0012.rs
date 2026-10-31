//! Ordering without a language reads every language and not a list.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, ListProperty, NgsiLdMediaType, Value},
    query::q,
};

/// Pins: q name`[*]`>"S" matches the entity one of whose language strings is greater than S, and never a
/// `ListProperty`
/// Clause: 4.9
/// Recorded: clause 4.9 makes any string of the `languageMap` the target value when no language is named but does
/// not say how two strings are ordered; Andromeda compares them by character code. The `ListProperty`, whose target
/// value is an array, does not match (clause 4.9, greater: no equality between the data types is not a match)
#[gtest]
#[tokio::test]
async fn o_qry_qlg_0012_ordering_without_a_language_reads_every_language_and_not_a_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_type = namespace.entity_type("Building")?;
    let building_gate_id = namespace.urn("Building", "gate")?;
    let building_list_id = namespace.urn("Building", "list")?;
    let building_door = Entity::builder()
        .id(building_door_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Tuer")])?),
        )
        .build()?;
    broker.entities().create(&building_door).send().await?.done()?;
    let building_gate = Entity::builder()
        .id(building_gate_id)
        .entity_type(building_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Gate"), ("de", "Dach")])?),
        )
        .build()?;
    broker.entities().create(&building_gate).send().await?.done()?;
    let building_list = Entity::builder()
        .id(building_list_id)
        .entity_type(building_type.clone())
        .list_property("name", ListProperty::builder(vec![Value::from("Tuer")]))
        .build()?;
    broker.entities().create(&building_list).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("name").member("*").gt("S"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_door_id)));
    Ok(())
}
