//! Local scope alone is enough and a geometry property without geojson is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, received_status::ExpectedStatus, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Create `EntityMap` on broker A with local true and nothing else answers 201 with an `EntityMap`, and by
/// type with `geometryProperty` location and Accept application/json answers 400 `BadRequestData`
/// Clause: 5.14.4.4, 5.5.13
#[gtest]
#[tokio::test]
async fn n_map_qen_7604_local_scope_alone_is_enough_and_a_geometry_property_without_geojson_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker
        .with_heads()
        .with_accept(Accept::any())
        .entity_maps()
        .create()
        .locality(Locality::Local)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?type=Vehicle_{namespace}&geometryProperty=location"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
