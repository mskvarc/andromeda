//! Query with an unmatched language answers the none entry.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Query Entities with lang=de answers a `LanguageProperty` that holds no de entry but an @none entry as a
/// Property carrying the @none string and the lang @none
/// Clause: 5.7.2.5, 4.15
#[gtest]
#[tokio::test]
async fn n_qry_lng_4001_query_with_an_unmatched_language_answers_the_none_entry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "red"), ("@none", "x")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // 5.7.2.5: "If no match can be made for a LanguageProperty, then the default identified by the JSON-LD "@none" shall be chosen if present".
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .lang("de".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(0, all!(has_id(&building_id), has_attribute("label", property().value("x").lang("@none"))))
    );
    Ok(())
}
