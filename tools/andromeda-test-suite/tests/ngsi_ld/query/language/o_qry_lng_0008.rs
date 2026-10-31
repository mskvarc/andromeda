//! Retrieve with a zero quality language leaves that language out.

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

/// Pins: Retrieve Entity with lang=fr;q=0,nl answers the nl entry: a quality value of zero takes fr out of the
/// preferences
/// Clause: 4.15, 5.7.1.5
/// Recorded: clause 4.15 uses the quality values of IETF RFC 3282 without stating that q=0 means not acceptable
/// (IETF RFC 7231 section 5.3.1 does); Andromeda excludes the language
#[gtest]
#[tokio::test]
async fn o_qry_lng_0008_retrieve_with_a_zero_quality_language_leaves_that_language_out() -> Result<()> {
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
        .lang("fr;q=0,nl".parse::<LanguageFilter>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "street"), some(eq(3)));
    expect_that!(entity, has_attribute("street", property().value("Grote Markt").lang("nl")));
    Ok(())
}
