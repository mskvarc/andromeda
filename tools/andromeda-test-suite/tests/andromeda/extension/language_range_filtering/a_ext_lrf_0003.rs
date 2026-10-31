//! Ordering by a range is bad request data and ordering by a tag is accepted.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::ordering::OrderBy,
};

/// Pins: at the language-ranges level, Query Entities with orderBy=label`[de-*-DE]` answers `BadRequestData`
/// because a range orders by no one value, and orderBy=label`[en-GB]` answers 200 with both entities
/// Clause: 4.9, 4.5.18, 5.7.2.4
/// Recorded: Andromeda extension, not v1.9.1: the language-ranges level of the `q_target_languages` extension reads
/// a q language selector as an RFC 4647 extended range matched by section 3.3.2 extended filtering, where clause
/// 4.9 compares a target language as one tag
#[gtest]
#[tokio::test]
async fn a_ext_lrf_0003_ordering_by_a_range_is_bad_request_data_and_ordering_by_a_tag_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_range = broker::client(BrokerRole::Edge)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor = Entity::builder()
        .id(sensor_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker_language_range.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Escalator")])?))
        .build()?;
    broker_language_range.entities().create(&sensor_2).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker_language_range.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&orderBy=label%5Bde-%2A-DE%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_language_range
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send_unchecked(request)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let page = broker_language_range
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .ordering("label[en-GB]".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    Ok(())
}
