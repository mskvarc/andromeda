//! Query with a language answers each language property as a property.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute, member_count},
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

/// Pins: Query Entities with lang=nl answers each `LanguageProperty` as a Property carrying the Dutch string and
/// its lang
/// Clause: 4.15, 5.7.2.5
#[gtest]
#[tokio::test]
async fn n_qry_lng_0018_query_with_a_language_answers_each_language_property_as_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_id.clone()])
        .lang("nl".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first().map(member_count), some(eq(3)));
    expect_that!(page.items().first().and_then(|item| attribute_members(item, "street")), some(eq(3)));
    expect_that!(page.items(), nth(0, has_attribute("street", property().value("Grote Markt").lang("nl"))));
    Ok(())
}
