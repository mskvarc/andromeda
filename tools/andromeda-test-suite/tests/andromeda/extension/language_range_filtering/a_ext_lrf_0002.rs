//! A wildcard subtag range matches by extended filtering.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    unchecked_page::entities,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: at the language-ranges level, label`[de-*-DE]` selects the tags de-DE and de-Latn-DE and not de-Deva,
/// label`[*-GB]` selects en-GB and de-Latn-GB and not en-US, and label`[*]` still selects every language
/// Clause: 4.9, 4.5.18
/// Recorded: Andromeda extension, not v1.9.1: the language-ranges level of the `q_target_languages` extension reads
/// a q language selector as an RFC 4647 extended range matched by section 3.3.2 extended filtering, where clause
/// 4.9 compares a target language as one tag; RFC 4647 section 3.3.2 lets a * subtag stand for any number of
/// subtags
#[gtest]
#[tokio::test]
async fn a_ext_lrf_0002_a_wildcard_subtag_range_matches_by_extended_filtering() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_range = broker::client(BrokerRole::Edge)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let mut sensor_ids = Vec::new();
    for (index, tag) in ["de-DE", "de-Latn-DE", "de-Deva", "en-GB", "de-Latn-GB", "en-US"].into_iter().enumerate() {
        let sensor_id = namespace.urn("Sensor", index + 1)?;
        let sensor = Entity::builder()
            .id(sensor_id.clone())
            .entity_type(sensor_type.clone())
            .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([(tag, "Lift")])?))
            .build()?;
        broker_language_range.entities().create(&sensor).send().await?.done()?;
        sensor_ids.push(sensor_id);
    }
    // The model refuses a language range with a wildcard subtag, so the query is sent as written.
    let request = request_to(
        &broker_language_range,
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=label%5Bde-%2A-DE%5D%3D%3D%22Lift%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_language_range.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let found = entities(answer.as_ref().or_fail()?)?;
    expect_that!(found, len(eq(2)));
    expect_that!(found, contains(has_id(&sensor_ids[0])));
    expect_that!(found, contains(has_id(&sensor_ids[1])));
    // The model refuses a language range with a wildcard subtag, so the query is sent as written.
    let request_2 = request_to(
        &broker_language_range,
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=label%5B%2A-GB%5D%3D%3D%22Lift%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker_language_range.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::OK));
    let found_2 = entities(answer_2.as_ref().or_fail()?)?;
    expect_that!(found_2, len(eq(2)));
    expect_that!(found_2, contains(has_id(&sensor_ids[3])));
    expect_that!(found_2, contains(has_id(&sensor_ids[4])));
    let page = broker_language_range
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("*").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(6)));
    for sensor_id in &sensor_ids {
        expect_that!(page.items(), contains(has_id(sensor_id)));
    }
    Ok(())
}
