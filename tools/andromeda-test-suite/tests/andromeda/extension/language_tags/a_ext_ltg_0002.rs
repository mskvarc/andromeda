//! Script and numeric region tags are valid selectors and match.

use andromeda_test_suite::{broker, config::BrokerRole, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: at the language-tags level, Query Entities with q label`[zh-Hant-TW]`=="Lift" and label`[es-419]`=="Lift"
/// answers 200 and selects the entity whose `languageMap` holds the operand under that exact tag
/// Clause: 4.9, 4.5.18
/// Recorded: Andromeda extension, not v1.9.1: the language-tags level of the `q_target_languages` extension admits
/// every RFC 5646 tag as a q target language, which the clause 4.9 `AttrName` cannot spell, and RFC 5646 section
/// 2.1.1 makes tags compare case-insensitively
#[gtest]
#[tokio::test]
async fn a_ext_ltg_0002_script_and_numeric_region_tags_are_valid_selectors_and_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_tags = broker::client(BrokerRole::Prefixed)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("zh-Hant-TW", "Lift"), ("es-419", "Ascensor")])?),
        )
        .build()?;
    broker_language_tags.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("zh-Hant-CN", "Lift"), ("es-ES", "Ascensor")])?),
        )
        .build()?;
    broker_language_tags.entities().create(&sensor_2).send().await?.done()?;
    let page = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("zh-Hant-TW").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let page_2 = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("es-419").eq("Ascensor"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_id)));
    let page_3 = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("es-419").eq("Lift"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
