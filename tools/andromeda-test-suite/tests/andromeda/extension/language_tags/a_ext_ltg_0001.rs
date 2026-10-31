//! A region tag selector matches a map key in either case.

use andromeda_test_suite::{broker, config::BrokerRole, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: at the language-tags level, Query Entities with q label`[en-GB]`=="Lift" answers the entities whose
/// `languageMap` holds the operand under the key en-GB and under the key en-gb, and not the entity holding it under
/// en-US
/// Clause: 4.9, 4.5.18, C.5.13
/// Recorded: Andromeda extension, not v1.9.1: the language-tags level of the `q_target_languages` extension admits
/// every RFC 5646 tag as a q target language, which the clause 4.9 `AttrName` cannot spell, and RFC 5646 section
/// 2.1.1 makes tags compare case-insensitively
#[gtest]
#[tokio::test]
async fn a_ext_ltg_0001_a_region_tag_selector_matches_a_map_key_in_either_case() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_tags = broker::client(BrokerRole::Prefixed)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_3_id = namespace.urn("Sensor", 3)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-gb", "Lift")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor_2).send().await?.done()?;
    let sensor_3 = Entity::builder()
        .id(sensor_3_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-US", "Lift")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor_3).send().await?.done()?;
    let page = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en-GB").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&sensor_id)));
    expect_that!(page.items(), contains(has_id(&sensor_2_id)));
    Ok(())
}
