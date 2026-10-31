//! Numbers beyond the numeric range are refused naming the limit.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution whose instances hold numbers beyond the numeric range (fractional or integral) is
/// refused with 400 `BadRequestData` naming the first broken limit in document order, and no Temporal Evolution is
/// created
/// Clause: 5.6.11.4, 4.5.1
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM 009
/// v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others naming the broken limit
#[gtest]
#[tokio::test]
async fn o_tmp_ups_0006_numbers_beyond_the_numeric_range_are_refused_naming_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (a number outside the range of a double: 1e-16384), so `speed` is written as text.
    let speed: RawJson = r#"[{"type": "Property", "value": 1e-16384, "observedAt": "2020-08-01T12:03:00Z"}, {"type": "Property", "value": 1e200000, "observedAt": "2020-08-01T12:05:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /speed/0/value is refused: the number needs more than 16383 digits after the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it"));
    let vehicle_evolution_2 = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (a number outside the range of a double: 1e200000), so `speed` is written as text.
    let speed_2: RawJson = r#"[{"type": "Property", "value": 1e200000, "observedAt": "2020-08-01T12:05:00Z"}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(vehicle_evolution_2.render(Lossless::Normalized))?.set_member("speed", &speed_2);
    let request_2 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution_2).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /speed/0/value is refused: the number needs more than 131072 digits before the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it"));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    Ok(())
}
