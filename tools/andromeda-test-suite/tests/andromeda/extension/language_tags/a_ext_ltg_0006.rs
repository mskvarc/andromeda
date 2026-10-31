//! A regional key matches whatever case the selector spells.

use andromeda_test_suite::{broker, config::BrokerRole, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: at the language-tags level, an entity whose languageMap keys the string under it-IT is selected by label[it-it], by label[IT-IT] and by label[it-IT] alike, and an entity keyed under another language is not selected
/// Clause: 4.9, 4.5.18, RFC 5646 section 2.1.1
/// Recorded: Andromeda extension, not v1.9.1: the language-tags level of the `q_target_languages` extension admits every RFC 5646 tag as a q target language, which the clause 4.9 `AttrName` cannot spell; RFC 5646 section 2.1.1 makes tags compare case-insensitively, and a tag is ASCII, so the fold is ASCII-only and holds on a database whose default locale maps I to the dotless ı
#[gtest]
#[tokio::test]
async fn a_ext_ltg_0006_a_regional_key_matches_whatever_case_the_selector_spells() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_tags = broker::client(BrokerRole::LanguageTags)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("it-IT", "Ciao")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Ciao")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor_2).send().await?.done()?;
    for selector in ["it-it", "IT-IT", "it-IT"] {
        let page = broker_language_tags
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&sensor_type)
            .q(q::attr("label").member(selector).eq("Ciao"))
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    }
    Ok(())
}
