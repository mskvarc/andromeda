//! A projection through a linked entity without a join is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, received_status::ExpectedStatus, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create `EntityMap` for Query Entities with a pick through a Linked Entity term and no join answers 400
/// `BadRequestData`, while the same type selection without the pick answers 201
/// Clause: 5.14.4.4, 4.21, 4.5.23
#[gtest]
#[tokio::test]
async fn n_map_qen_7001_a_projection_through_a_linked_entity_without_a_join_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // The client refuses a projection through a linked entity without a join, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?type=Vehicle_{namespace}&pick=linkedTo%7Bspeed%7D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    Ok(())
}
