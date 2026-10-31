//! An observed at with nine fraction digits is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    nth::nth,
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

/// Pins: Add Attributes to Temporal Evolution whose instance carries an `observedAt` of nine fractional-second
/// digits answers 400 `BadRequestData` and adds no instance
/// Clause: 4.6.3, 5.6.12.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_7601_an_observed_at_with_nine_fraction_digits_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/{vehicle_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"speed\": [\n        { \"type\": \"Property\", \"value\": 2, \"observedAt\": \"2026-03-04T05:06:07.123456789Z\" }\n    ],\n    \"heading\": [\n        { \"type\": \"Property\", \"value\": 90, \"observedAt\": \"2026-01-01T00:00:00Z\" }\n    ]\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().value(1).observed_at("2026-01-01T00:00:00Z".parse()?)))
        )
    );
    expect_that!(evolution, lacks_evolution("heading"));
    Ok(())
}
