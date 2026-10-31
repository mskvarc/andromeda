//! Create temporal with a mixed arity line string is bad request.

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
        ErrorType,
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{LineString, Position, Shape},
    },
};

/// Pins: Create Temporal Evolution refuses a `GeoProperty` instance holding a `LineString` that mixes two- and
/// three-ordinate positions with a 400 and creates nothing
/// Clause: 4.7.1, 5.6.11, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected; no clause names the error type, so `BadRequestData` and
/// `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_2712_create_temporal_with_a_mixed_arity_line_string_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            GeoProperty::builder(Shape::LineString(LineString::try_new(vec![
                Position::try_new(13.4, 52.52)?,
                Position::try_with_altitude(13.5, 52.53, 250.0)?,
            ])?))
            .observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .build()?;
    let answer = broker.with_responses().temporal().create_or_update(&building_evolution).send().await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
