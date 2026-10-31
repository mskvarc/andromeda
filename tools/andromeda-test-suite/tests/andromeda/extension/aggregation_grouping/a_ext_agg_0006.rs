//! Misused grouping parameters are refused.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: `aggrGroupBy` beside `format=aggregatedValues` is refused with `InvalidRequest`, and
/// `format=groupedValues` without `aggrMethods` is refused with `BadRequestData`
/// Clause: 4.5.19, 6.3.20, 6.19.3.1
/// Recorded: Andromeda extension, not v1.9.1: the temporal binding defines neither `aggrGroupBy` nor the
/// `groupedValues` format; the `temporal_aggregation_grouping` extension adds them, here with at most 3 groups
#[gtest]
#[tokio::test]
async fn a_ext_agg_0006_misused_grouping_parameters_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(10)
                .unit_code(UnitCode::Kmh)
                .dataset_id(namespace.dataset_id("Dataset", "A")?)
                .observed_at("2024-01-01T08:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(20)
                .unit_code(UnitCode::Kmh)
                .dataset_id(namespace.dataset_id("Dataset", "A")?)
                .observed_at("2024-02-01T09:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(30)
                .unit_code(UnitCode::Hm)
                .dataset_id(namespace.dataset_id("Dataset", "B")?)
                .observed_at("2024-01-15T08:30:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(40)
                .unit_code(UnitCode::Hm)
                .dataset_id(namespace.dataset_id("Dataset", "B")?)
                .observed_at("2024-03-01T10:00:00Z".parse()?),
        )
        .property("zone", Property::builder("north").observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("zone", Property::builder("south").observed_at("2024-02-15T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities/{vehicle_id}?timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&attrs=speed&format=aggregatedValues&aggrMethods=avg&aggrGroupBy=unitCode"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities/{vehicle_id}?timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&attrs=speed&format=groupedValues&aggrGroupBy=unitCode"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
