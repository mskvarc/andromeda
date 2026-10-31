//! A regional variant matches a wildcard selector in either case.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
    unchecked_page::entities,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::{Entity, LanguageMap, LanguageProperty};

/// Pins: at the language-ranges level, an entity whose languageMap keys the string under it-IT is selected by the
/// extended range label[it-*] and by label[IT-*], and an entity keyed under another language is not selected
/// Clause: 4.9, 4.5.18, RFC 4647 section 3.3.2
/// Recorded: Andromeda extension, not v1.9.1: the language-ranges level of the `q_target_languages` extension reads a
/// q language selector as an RFC 4647 extended range matched by section 3.3.2 extended filtering, where clause 4.9
/// compares a target language as one tag; RFC 4647 section 2 compares subtags case-insensitively, and a range is
/// ASCII, so the fold is ASCII-only and holds on a database whose default locale maps I to the dotless ı
#[gtest]
#[tokio::test]
async fn a_ext_lrf_0006_a_regional_variant_matches_a_wildcard_selector_in_either_case() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_range = broker::client(BrokerRole::LanguageRange)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("it-IT", "Ciao")])?))
        .build()?;
    broker_language_range.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type)
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Ciao")])?))
        .build()?;
    broker_language_range.entities().create(&sensor_2).send().await?.done()?;
    for range in ["it-%2A", "IT-%2A"] {
        // The model refuses a language range with a wildcard subtag, so the query is sent as written.
        let request = request_to(
            &broker_language_range,
            Method::GET,
            &format!("entities?type=Sensor_{namespace}&q=label%5B{range}%5D%3D%3D%22Ciao%22"),
        )?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = broker_language_range.send_unchecked(request).await;
        expect_that!(&answer, answered(StatusCode::OK));
        let found = entities(answer.as_ref().or_fail()?)?;
        expect_that!(found, len(eq(1)));
        expect_that!(found, nth(0, has_id(&sensor_id)));
    }
    Ok(())
}
