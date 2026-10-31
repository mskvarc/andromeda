//! An observed at with a numeric offset is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Replace Attribute whose replacement carries an `observedAt` with a numeric offset in place of Z (+01:00,
/// or +00:00 naming UTC) answers 400 `BadRequestData` and leaves the Attribute unchanged
/// Clause: 4.6.3, 5.6.19.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_att_rpl_7604_an_observed_at_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PUT, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(
            "{\n    \"type\": \"Property\",\n    \"value\": 2,\n    \"observedAt\": \"2026-03-04T05:06:07+01:00\"\n}",
        ));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this attribute, so the body is sent as written.
    let request_2 = request_to(&broker, Method::PUT, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(
            "{\n    \"type\": \"Property\",\n    \"value\": 2,\n    \"observedAt\": \"2026-03-04T05:06:07+00:00\"\n}",
        ));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("heading"));
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute().value(1).observed_at("2026-01-01T00:00:00Z".parse()?).without(Member::ExpiresAt)
        )
    );
    Ok(())
}
