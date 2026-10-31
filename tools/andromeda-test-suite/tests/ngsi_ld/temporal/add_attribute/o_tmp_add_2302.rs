//! Numbers beyond the numeric range are refused naming the limit.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution refuses a number beyond the numeric range, both a very large and a
/// very small exponent, with 400 `BadRequestData` naming the broken limit and writes nothing
/// Clause: 4.5.1, 5.6.12
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM 009
/// v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others naming the broken limit
#[gtest]
#[tokio::test]
async fn o_tmp_add_2302_numbers_beyond_the_numeric_range_are_refused_naming_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("brandName", Property::builder("Mercedes").observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/{vehicle_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\"big\": [{\"type\": \"Property\", \"value\": 1e200000, \"observedAt\": \"2020-08-01T13:00:00Z\"}], \"tiny\": [{\"type\": \"Property\", \"value\": 1e-16384, \"observedAt\": \"2020-08-01T13:00:00Z\"}]}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /big/0/value is refused: the number needs more than 131072 digits before the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it"));
    // The model refuses this temporal fragment, so the body is sent as written.
    let request_2 = request_to(&broker, Method::POST, &format!("temporal/entities/{vehicle_id}/attrs"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(
            "{\"tiny\": [{\"type\": \"Property\", \"value\": 1e-16384, \"observedAt\": \"2020-08-01T13:00:00Z\"}]}",
        ));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /tiny/0/value is refused: the number needs more than 16383 digits after the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it"));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, lacks_evolution("big"));
    expect_that!(evolution, lacks_evolution("tiny"));
    expect_that!(evolution, has_evolution("brandName", len(eq(1))));
    Ok(())
}
