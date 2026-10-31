//! A full tag matches by tag equality and a range stays bad request data.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::{ordering::OrderBy, q},
};

/// Pins: at the language-tags level, label`[en-GB]` selects the key en-GB and not en-GB-oxendict, label`[en]` does
/// not select en-GB, orderBy=label`[en-GB]` answers 200, and label`[de-*-DE]` in q or `orderBy` still answers
/// `BadRequestData`
/// Clause: 4.9, 4.5.18, 4.23
/// Recorded: Andromeda extension, not v1.9.1: the language-tags level of the `q_target_languages` extension admits
/// every RFC 5646 tag as a q target language, which the clause 4.9 `AttrName` cannot spell, and keeps the clause
/// 4.9 tag equality; a wildcard range is read only at the language-ranges level
#[gtest]
#[tokio::test]
async fn a_ext_ltg_0003_a_full_tag_matches_by_tag_equality_and_a_range_stays_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_tags = broker::client(BrokerRole::Prefixed)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB-oxendict", "Lift")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor_2).send().await?.done()?;
    let page = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en-GB").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let page_2 = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en").eq("Lift"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .ordering("label[en-GB]".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker_language_tags.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=label%5Bde-%2A-DE%5D%3D%3D%22Lift%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send_unchecked(request)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker_language_tags.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&orderBy=label%5Bde-%2A-DE%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker_language_tags
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send_unchecked(request_2)
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
