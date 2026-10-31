//! A geo query coordinates parameter with mixed arity positions is refused.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem_among};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
    query::geoquery,
};

/// Pins: Query Entities with a geo-query whose coordinates mix two- and three-ordinate positions is refused with a
/// 400
/// Clause: 4.7.1, 4.10, 5.7.2.3
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry is nonsensical for a context broker and must be rejected on every path that
/// takes a geometry; no clause names the error type, so `BadRequestData` and `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_2308_a_geo_query_coordinates_parameter_with_mixed_arity_positions_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::LineString(LineString::try_new(vec![
            Position::try_new(13.4, 52.52)?,
            Position::try_with_altitude(13.5, 52.53, 250.0)?,
        ])?)))
        .entity_type(&namespace.entity_type("Building")?)
        .send()
        .await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    Ok(())
}
