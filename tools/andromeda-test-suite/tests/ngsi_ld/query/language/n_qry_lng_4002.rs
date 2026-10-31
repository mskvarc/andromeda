//! Query with an unmatched language list answers the none entry.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Query Entities with a lang list of de, fr and with a weighted list de;q=0.9,fr;q=0.5, none of which a
/// `languageMap` holds, answers the @none entry with the lang @none
/// Clause: 5.7.2.5, 4.15
#[gtest]
#[tokio::test]
async fn n_qry_lng_4002_query_with_an_unmatched_language_list_answers_the_none_entry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "red"), ("nl", "rood"), ("@none", "x")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .lang("de,fr".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_attribute("label", property().value("x").lang("@none"))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .lang("de;q=0.9,fr;q=0.5".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_attribute("label", property().value("x").lang("@none"))));
    Ok(())
}
