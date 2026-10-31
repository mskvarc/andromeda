//! An observed at with a numeric offset is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution whose instance carries an `observedAt` with a numeric offset in place
/// of Z (+01:00, or +00:00 naming UTC) answers 400 `BadRequestData`, and neither the Temporal Evolution nor the
/// Entity exists
/// Clause: 4.6.3, 5.6.11.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_2708_an_observed_at_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed: RawJson = r#"[{"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"}, {"type": "Property", "value": 2, "observedAt": "2026-03-04T05:06:07+01:00"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_evolution_2 = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed_2: RawJson = r#"[{"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"}, {"type": "Property", "value": 2, "observedAt": "2026-03-04T05:06:07+00:00"}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(vehicle_evolution_2.render(Lossless::Normalized))?.set_member("speed", &speed_2);
    let request_2 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution_2).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
