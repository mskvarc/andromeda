//! Retrieve with a language range matches a dialect by prefix.

use andromeda_test_suite::{
    attribute_matcher::property,
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

/// Pins: Retrieve Entity with lang=fr answers the fr-CA entry by language range matching when the `languageMap`
/// holds no fr entry
/// Clause: 4.15, 5.7.1.5
/// Recorded: clause 4.15 takes the filter grammar from IETF RFC 5646 and IETF RFC 3282 but does not say that a
/// range fr matches the tag fr-CA (basic filtering of IETF RFC 4647 does); Andromeda matches a range as a tag
/// prefix, and clause 5.7.1.5 would leave the entry chosen to the implementation otherwise
#[gtest]
#[tokio::test]
async fn o_qry_lng_0006_retrieve_with_a_language_range_matches_a_dialect_by_prefix() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr-CA", "Rue Principale"), ("de", "Hauptstrasse")])?),
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
    expect_that!(attribute_members(entity, "street"), some(eq(3)));
    expect_that!(entity, has_attribute("street", property().value("Rue Principale").lang("fr-CA")));
    Ok(())
}
