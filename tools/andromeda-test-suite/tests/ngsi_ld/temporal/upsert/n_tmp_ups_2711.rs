//! A declared time value not of its form on a sub attribute is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution whose instance holds a sub-attribute with a value declared as a
/// `DateTime` that is not of its form answers 400 `BadRequestData`, and neither the Temporal Evolution nor the
/// Entity exists
/// Clause: 4.6.3, 4.5.2.2, 5.6.11.4, 5.5.4, C.6
#[gtest]
#[tokio::test]
async fn n_tmp_ups_2711_a_declared_time_value_not_of_its_form_on_a_sub_attribute_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (invalid value at speed.calibratedAt), so `speed` is written as text.
    let speed: RawJson = r#"[{"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z", "calibratedAt": {"type": "Property", "value": {"@type": "DateTime", "@value": "2026-01-01T01:00:00+01:00"}}}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
