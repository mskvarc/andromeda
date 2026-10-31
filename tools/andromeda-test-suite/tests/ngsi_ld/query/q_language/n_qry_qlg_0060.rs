//! A bracket member that is no language tag is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with q label`[en_GB-x]`=="Lift", a bracket member that is neither an attribute name nor a
/// language tag, answers `BadRequestData` by GET and by POST
/// Clause: 4.9, 4.5.18
/// Recorded: RFC 5646 section 2.1 writes a tag from letters, digits and hyphens only, and an underscore is none of
/// them
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0060_a_bracket_member_that_is_no_language_tag_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en_GB-x").eq("Lift"))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en_GB-x").eq("Lift"))
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
