//! A line string mixing two and three ordinate positions is bad request.

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

/// Pins: Create Entity refuses a `LineString` whose first position has two ordinates and whose second has three
/// with a 400, and stores nothing
/// Clause: 4.7.1, 5.6.1, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own ("two or more elements"), so it permits a
/// geometry whose positions differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer
/// open; the user ruled on 2026-10-06 that such a geometry must be rejected as nonsensical for a context broker;
/// Table 5.5.2-1 and 5.5.4 name no error type for this case, so `BadRequestData` and `InvalidRequest` are both
/// accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_0064_a_line_string_mixing_two_and_three_ordinate_positions_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![
                Position::try_new(13.4, 52.52)?,
                Position::try_with_altitude(13.5, 52.53, 250.0)?,
            ])?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&building).send().await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    // Nothing was stored.
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
