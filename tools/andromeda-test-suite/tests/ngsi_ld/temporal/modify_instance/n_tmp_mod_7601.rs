//! An observed at with nine fraction digits is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at, lacks_evolution},
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

/// Pins: Modify Attribute instance whose replacement carries an `observedAt` of nine fractional-second digits
/// answers 400 `BadRequestData` and leaves the instance unchanged
/// Clause: 4.6.3, 5.6.14.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_mod_7601_an_observed_at_with_nine_fraction_digits_is_bad_request_data() -> Result<()> {
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
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance_id = instance_id_at(evolution, &"speed".parse()?, 0)?;
    // The model refuses this instance, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("temporal/entities/{vehicle_id}/attrs/speed/{instance_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(
            "{\n    \"speed\": [\n        { \"type\": \"Property\", \"value\": 2, \"observedAt\": \"2026-03-04T05:06:07.123456789Z\" }\n    ]\n}",
        ));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().value(1).observed_at("2026-01-01T00:00:00Z".parse()?)))
        )
    );
    expect_that!(evolution_2, lacks_evolution("heading"));
    Ok(())
}
