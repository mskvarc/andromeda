//! Updating a location to a mixed arity line string is bad request.

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
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        ErrorType,
        NgsiLdMediaType,
        Value,
        geometry::{LineString, Position, Shape},
    },
};

/// Pins: Update Attributes refuses a `LineString` mixing two- and three-ordinate positions with a 400 and leaves
/// the stored `LineString` unchanged
/// Clause: 4.7.1, 5.6.2, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own ("two or more elements"), so it permits a
/// geometry whose positions differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer
/// open; the user ruled on 2026-10-06 that such a geometry must be rejected as nonsensical for a context broker;
/// Table 5.5.2-1 and 5.5.4 name no error type for this case, so `BadRequestData` and `InvalidRequest` are both
/// accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_0065_updating_a_location_to_a_mixed_arity_line_string_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![Position::try_new(13.4, 52.52)?, Position::try_new(13.5, 52.53)?])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![
                Position::try_new(13.4, 52.52)?,
                Position::try_with_altitude(13.5, 52.53, 250.0)?,
            ])?),
        )
        .build()?;
    let answer = broker.with_responses().entities().update_attributes(&building_id, &fragment).send().await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    // The stored geometry is unchanged.
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .value_member(["type"], "LineString")
                .length_at(["coordinates"], 2)
                .length_at(["coordinates", "0"], 2)
                .length_at(["coordinates", "1"], 2)
                .value_member(["coordinates", "1", "0"], Value::Number("13.5".parse()?))
                .value_member(["coordinates", "1", "1"], Value::Number("52.53".parse()?))
        )
    );
    Ok(())
}
