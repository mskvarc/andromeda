//! A change is read back and missing targets are refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
};

/// Pins: Partial Attribute Update on broker A of an Entity it holds answers 204 and the change reads back; of
/// colour, which the Entity does not hold, it answers 404 `ResourceNotFound`
/// Clause: 5.6.4.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_att_pat_7602_a_change_is_read_back_and_missing_targets_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The concise rendering of a lone value is the bare value, so the body is sent as written.
    let colour = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/colour"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"value\": \"red\"\n}"));
    let answer = broker.send_unchecked(colour).await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The concise rendering of a lone value is the bare value, so the body is sent as written.
    let speed = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"value\": 60\n}"));
    let answer_2 = broker.send_unchecked(speed).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(60)));
    Ok(())
}
