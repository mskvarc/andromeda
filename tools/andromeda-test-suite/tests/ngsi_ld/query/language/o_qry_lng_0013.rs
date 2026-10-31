//! Retrieve with a language keeps the sub attributes.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
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

/// Pins: Retrieve Entity with lang=fr keeps the sub-attributes of the converted `LanguageProperty`
/// Clause: 4.15, 5.7.1.5
/// Recorded: clauses 4.15 and 5.7.1.5 convert the `LanguageProperty` into a Property and add a lang sub-property,
/// and say nothing of the sub-attributes the attribute already holds; Andromeda keeps them
#[gtest]
#[tokio::test]
async fn o_qry_lng_0013_retrieve_with_a_language_keeps_the_sub_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?).property("source", "OpenStreetMap"),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .lang("fr".parse::<LanguageFilter>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "street"), some(eq(4)));
    expect_that!(
        entity,
        has_attribute(
            "street",
            property()
                .value("Grand Place")
                .lang("fr")
                .without(Member::LanguageMap)
                .sub_attribute("source", property().value("OpenStreetMap"))
        )
    );
    Ok(())
}
