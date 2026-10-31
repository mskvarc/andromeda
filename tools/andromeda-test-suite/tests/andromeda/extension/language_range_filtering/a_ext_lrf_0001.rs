//! A bare language range matches the language and its regional variants.

use andromeda_test_suite::{broker, config::BrokerRole, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: at the language-ranges level, Query Entities with q label`[en]`=="Lift" answers the entities whose
/// `languageMap` holds the operand under en, en-GB and en-gb, and not the entity holding it under fr
/// Clause: 4.9, 4.5.18
/// Recorded: Andromeda extension, not v1.9.1: the language-ranges level of the `q_target_languages` extension reads
/// a q language selector as an RFC 4647 extended range matched by section 3.3.2 extended filtering, where clause
/// 4.9 compares a target language as one tag
#[gtest]
#[tokio::test]
async fn a_ext_lrf_0001_a_bare_language_range_matches_the_language_and_its_regional_variants() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_range = broker::client(BrokerRole::LanguageRange)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_3_id = namespace.urn("Sensor", 3)?;
    let sensor_4_id = namespace.urn("Sensor", 4)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Lift")])?))
        .build()?;
    broker_language_range.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker_language_range.entities().create(&sensor_2).send().await?.done()?;
    let sensor_3 = Entity::builder()
        .id(sensor_3_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-gb", "Lift")])?))
        .build()?;
    broker_language_range.entities().create(&sensor_3).send().await?.done()?;
    let sensor_4 = Entity::builder()
        .id(sensor_4_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Lift")])?))
        .build()?;
    broker_language_range.entities().create(&sensor_4).send().await?.done()?;
    let page = broker_language_range
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(has_id(&sensor_id)));
    expect_that!(page.items(), contains(has_id(&sensor_2_id)));
    expect_that!(page.items(), contains(has_id(&sensor_3_id)));
    Ok(())
}
