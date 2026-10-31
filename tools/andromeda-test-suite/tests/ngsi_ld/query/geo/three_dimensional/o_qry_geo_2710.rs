//! Partial update with a mixed arity line string is bad request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem_among,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
};

/// Pins: Partial Attribute Update refuses a `LineString` mixing two- and three-ordinate positions with a 400 and
/// leaves the stored `LineString` unchanged
/// Clause: 4.7.1, 5.6.4, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected; no clause names the error type, so `BadRequestData` and
/// `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_2710_partial_update_with_a_mixed_arity_line_string_is_bad_request() -> Result<()> {
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
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{building_id}/attrs/location"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(
            "{\n    \"value\": { \"type\": \"LineString\", \"coordinates\": [[13.4, 52.52], [13.5, 52.53, 250.0]] }\n}",
        ));
    let answer = broker.send_unchecked(request).await;
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
