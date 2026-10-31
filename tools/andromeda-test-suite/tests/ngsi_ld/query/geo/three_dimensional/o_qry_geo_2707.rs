//! Append with a mixed arity line string is refused.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
};

/// Pins: Append Entity Attributes refuses a `LineString` mixing two- and three-ordinate positions without answering
/// 204 and leaves the stored `LineString` unchanged
/// Clause: 4.7.1, 5.6.3, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected; no clause names the error type
#[gtest]
#[tokio::test]
async fn o_qry_geo_2707_append_with_a_mixed_arity_line_string_is_refused() -> Result<()> {
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
    let answer = broker.with_responses().entities().append_attributes(&building_id, &fragment).send().await;
    expect_that!(answer.answered_status().map(|status| status.as_u16()), some(all!(ge(207), not(eq(204)))));
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
