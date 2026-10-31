//! Create with a multi line string mixing two and three ordinate positions is bad request.

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

/// Pins: Create Entity refuses a `MultiLineString` whose positions mix two and three ordinates with a 400 and
/// stores nothing
/// Clause: 4.7.1, 5.6.1, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected as nonsensical for a context broker; no clause names the error
/// type, so `BadRequestData` and `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_2702_create_with_a_multi_line_string_mixing_two_and_three_ordinate_positions_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            Shape::MultiLineString(vec![
                LineString::try_new(vec![Position::try_new(0.0, 0.0)?, Position::try_new(1.0, 1.0)?])?,
                LineString::try_new(vec![Position::try_with_altitude(2.0, 2.0, 5.0)?, Position::try_with_altitude(3.0, 3.0, 5.0)?])?,
            ]),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&building).send().await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
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
