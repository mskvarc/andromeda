//! Create with a line string mixing three then two ordinate positions is bad request.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{problem, problem_among},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
};

/// Pins: Create Entity refuses a `LineString` whose first position has three ordinates and whose second has two,
/// and a custom `GeoProperty` and an `observationSpace` holding such a geometry, each with a 400
/// Clause: 4.7.1, 5.6.1, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected as nonsensical for a context broker; no clause names the error
/// type, so `BadRequestData` and `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_2302_create_with_a_line_string_mixing_three_then_two_ordinate_positions_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_location_id = namespace.urn("Building", "location")?;
    let building_type = namespace.entity_type("Building")?;
    let building_custom_id = namespace.urn("Building", "custom")?;
    let building_observation_id = namespace.urn("Building", "observation")?;
    let building_location = Entity::builder()
        .id(building_location_id.clone())
        .entity_type(building_type.clone())
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![
                Position::try_with_altitude(13.4, 52.52, 250.0)?,
                Position::try_new(13.5, 52.53)?,
            ])?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&building_location).send().await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let building_custom = Entity::builder()
        .id(building_custom_id)
        .entity_type(building_type.clone())
        .geo_property(
            "route",
            Shape::LineString(LineString::try_new(vec![
                Position::try_new(13.4, 52.52)?,
                Position::try_with_altitude(13.5, 52.53, 250.0)?,
            ])?),
        )
        .build()?;
    let answer_2 = broker.with_responses().entities().create(&building_custom).send().await;
    expect_that!(
        &answer_2,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let building_observation = Entity::builder()
        .id(building_observation_id)
        .entity_type(building_type.clone())
        .geo_property(
            "observationSpace",
            Shape::LineString(LineString::try_new(vec![
                Position::try_new(13.4, 52.52)?,
                Position::try_with_altitude(13.5, 52.53, 250.0)?,
            ])?),
        )
        .build()?;
    let answer_3 = broker.with_responses().entities().create(&building_observation).send().await;
    expect_that!(
        &answer_3,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_location_id)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
