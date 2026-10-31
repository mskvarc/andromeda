//! An end time at with seven fraction digits is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Query Temporal Evolution of Entities with timerel between and an `endTimeAt` of seven fractional-second
/// digits answers 400 `BadRequestData`
/// Clause: 4.6.3, 4.11, 5.7.4.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_4011_an_end_time_at_with_seven_fraction_digits_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=between&timeAt=2026-01-01T00%3A00%3A00Z&endTimeAt=2026-03-04T05%3A06%3A07.1234567Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
