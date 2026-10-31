//! Query with an unmatched language and key values answers the none string.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, outcome::Done, simplified_matcher::has_value};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Query Entities with lang=de and options=simplified or `options=keyValues` answers the @none string of a
/// `LanguageProperty` that holds no de entry, and the de string of one that holds it
/// Clause: 5.7.2.5, 4.15, 4.5.1
#[gtest]
#[tokio::test]
async fn n_qry_lng_4003_query_with_an_unmatched_language_and_key_values_answers_the_none_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "red"), ("@none", "x")])?),
        )
        .language_property(
            "title",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("de", "Turm"), ("@none", "t")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .entity_type(&building_type)
        .lang("de".parse::<LanguageFilter>()?)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, all!(has_value("label", "x"), has_value("title", "Turm"))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .entity_type(&building_type)
        .lang("de".parse::<LanguageFilter>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, all!(has_value("label", "x"), has_value("title", "Turm"))));
    Ok(())
}
