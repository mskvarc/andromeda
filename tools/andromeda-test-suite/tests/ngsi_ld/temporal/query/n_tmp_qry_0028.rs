//! Limit zero without count is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Query Temporal Evolution of Entities with limit 0 and no count answers 400 `BadRequestData`
/// Clause: 6.3.10, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0028_limit_zero_without_count_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_0000_id = namespace.urn("Sensor", "0000")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0000_evolution = TemporalEntity::builder()
        .id(sensor_0000_id)
        .entity_type(sensor_type)
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0000_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Sensor_{namespace}&local=true&timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&limit=0"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
