//! Retrieve without a language answers the whole language map.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    language_map_matcher::has_language_map,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
};

/// Pins: Retrieve Entity without lang answers a `LanguageProperty` with its whole `languageMap`
/// Clause: 4.5.18.2, 5.7.1.5
#[gtest]
#[tokio::test]
async fn n_qry_lng_0012_retrieve_without_a_language_answers_the_whole_language_map() -> Result<()> {
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
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "street"), some(eq(2)));
    expect_that!(entity, has_attribute("street", attribute_of(AttributeKind::LanguageProperty)));
    expect_that!(entity, has_language_map("street", &[("fr", "Grand Place"), ("nl", "Grote Markt")]));
    Ok(())
}
