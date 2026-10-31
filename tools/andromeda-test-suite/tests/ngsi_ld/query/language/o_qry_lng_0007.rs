//! Retrieve prefers an exact language match over a dialect.

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

/// Pins: Retrieve Entity with lang=fr answers the fr entry rather than the fr-CA entry when the `languageMap` holds
/// both
/// Clause: 4.15, 5.7.1.5
/// Recorded: clause 4.15 gives no precedence between an exact tag and a longer tag the range also matches;
/// Andromeda prefers the exact match
#[gtest]
#[tokio::test]
async fn o_qry_lng_0007_retrieve_prefers_an_exact_language_match_over_a_dialect() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr-CA", "Rue Principale"), ("fr", "Grand Place")])?),
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
    expect_that!(entity, has_attribute("street", property().value("Grand Place").lang("fr")));
    Ok(())
}
