//! Numbers beyond the numeric range are refused naming the limit.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Upsert answers 207 with a `BadRequestData` error naming the broken limit for each entity
/// holding a number beyond the numeric range, both a very large and a very small exponent, while upserting the
/// valid entity
/// Clause: 4.5.1, 5.6.8
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM 009
/// v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others naming the broken limit
#[gtest]
#[tokio::test]
async fn o_bat_ups_2302_numbers_beyond_the_numeric_range_are_refused_naming_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (a number outside the range of a double: 1e200000), so `big` is written as text.
    let big: RawJson = r#"{"type": "Property", "value": 1e200000}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("big", &big);
    let vehicle_2 = Entity::builder().id(vehicle_2_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (a number outside the range of a double: 1e-16384), so `tiny` is written as text.
    let tiny: RawJson = r#"{"type": "Property", "value": 1e-16384}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_2, Lossless::Normalized).set_member("tiny", &tiny);
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("big", 1)
        .build()?;
    let document_3 = UncheckedDocument::from_entity(&vehicle_3, Lossless::Normalized);
    let entities = [vehicle, vehicle_2, vehicle_3];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[document, document_2, document_3]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&vehicle_3_id)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&vehicle_id).problem(ErrorType::BadRequestData).with_detail("the JSON number at /big/value is refused: the number needs more than 131072 digits before the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")));
    expect_that!(&report.errors, nth(1, batch_error().entity(&vehicle_2_id).problem(ErrorType::BadRequestData).with_detail("the JSON number at /tiny/value is refused: the number needs more than 16383 digits after the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_3_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("big", any_attribute().value(1)));
    Ok(())
}
