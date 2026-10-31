//! An identifier alone is too wide and local scope is enough.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, received_status::ExpectedStatus, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Create `EntityMap` for Query Entities with only an Entity id answers 400 `BadRequestData` as too wide, and
/// with only local=true answers 201
/// Clause: 5.14.4.4, 5.5.13
#[gtest]
#[tokio::test]
async fn n_map_qen_0005_an_identifier_alone_is_too_wide_and_local_scope_is_enough() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .locality(Locality::Local)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    Ok(())
}
