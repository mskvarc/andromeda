//! An expires at with a numeric offset is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Update `EntityMap` with an `expiresAt` carrying a numeric offset in place of Z (+01:00, or +00:00 naming
/// UTC) answers 400 `BadRequestData` and leaves the `EntityMap` and its `expiresAt` unchanged
/// Clause: 4.6.3, 5.14.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_map_upd_7604_an_expires_at_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    // The body is no expiry the model writes, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entityMaps/{map_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"expiresAt\": \"2099-03-04T05:06:07+01:00\"\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The body is no expiry the model writes, so it is sent as written.
    let request_2 = request_to(&broker, Method::PATCH, &format!("entityMaps/{map_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"expiresAt\": \"2099-03-04T05:06:07+00:00\"\n}"));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let map = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map.id, eq(map_id));
    expect_that!(&map.expires_at, eq(&created.body().expires_at));
    Ok(())
}
