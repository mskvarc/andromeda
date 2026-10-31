//! Replace entity with a mixed arity line string is bad request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem_among,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityIdMember},
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
};

/// Pins: Replace Entity refuses a `LineString` mixing two- and three-ordinate positions with a 400 and leaves the
/// stored `LineString` unchanged
/// Clause: 4.7.1, 5.6.18, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected; no clause names the error type, so `BadRequestData` and
/// `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_2709_replace_entity_with_a_mixed_arity_line_string_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![Position::try_new(13.4, 52.52)?, Position::try_new(13.5, 52.53)?])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let replacement = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![
                Position::try_new(13.4, 52.52)?,
                Position::try_with_altitude(13.5, 52.53, 250.0)?,
            ])?),
        )
        .build()?;
    let answer = broker
        .with_responses()
        .entities()
        .replace(&replacement)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("location", any_attribute().length_at(["coordinates", "1"], 2)));
    Ok(())
}
