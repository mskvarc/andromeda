//! Numbers beyond the numeric range are refused naming the limit.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: a Batch Entity Creation keeps digit for digit a fraction and an integer beyond binary64 precision, and
/// answers 207 with a `BadRequestData` error naming the broken limit for each entity holding a number beyond the
/// numeric range (fractional or integral) while creating the valid entities
/// Clause: 5.6.7.4, 6.14.3.1, 4.5.1
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM 009
/// v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others naming the broken limit
#[gtest]
#[tokio::test]
async fn o_bat_cre_0005_numbers_beyond_the_numeric_range_are_refused_naming_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_fraction_id = namespace.urn("Building", "fraction")?;
    let building_type = namespace.entity_type("Building")?;
    let building_integer_id = namespace.urn("Building", "integer")?;
    let building_small_id = namespace.urn("Building", "small")?;
    let building_valid_id = namespace.urn("Building", "valid")?;
    let building_large_id = namespace.urn("Building", "large")?;
    let entities = [
        Entity::builder()
            .id(building_fraction_id.clone())
            .entity_type(building_type.clone())
            .property("reading", Value::Number("0.180000000000000005".parse()?))
            .build()?,
        Entity::builder()
            .id(building_integer_id.clone())
            .entity_type(building_type.clone())
            .property("reading", Value::Number("18446744073709551616".parse()?))
            .build()?,
    ];
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
    expect_that!(created, len(eq(2)));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_fraction_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("0.180000000000000005"));
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_integer_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received_2.body), contains_substring("18446744073709551616"));
    let building_small = Entity::builder().id(building_small_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses it (a number outside the range of a double: 1e-16384), so `reading` is written as text.
    let reading: RawJson = r#"{"type": "Property", "value": 1e-16384}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_small, Lossless::Normalized).set_member("reading", &reading);
    let building_valid = Entity::builder()
        .id(building_valid_id.clone())
        .entity_type(building_type.clone())
        .property("reading", 1)
        .build()?;
    let document_2 = UncheckedDocument::from_entity(&building_valid, Lossless::Normalized);
    let building_large = Entity::builder().id(building_large_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses it (a number outside the range of a double: 1e200000), so `reading` is written as text.
    let reading_2: RawJson = r#"{"type": "Property", "value": 1e200000}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_large, Lossless::Normalized).set_member("reading", &reading_2);
    let entities_2 = [building_small, building_valid, building_large];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities_2).build())?
        .documents(&[document, document_2, document_3]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_valid_id)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&building_small_id).problem(ErrorType::BadRequestData).with_detail("the JSON number at /reading/value is refused: the number needs more than 16383 digits after the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")));
    expect_that!(&report.errors, nth(1, batch_error().entity(&building_large_id).problem(ErrorType::BadRequestData).with_detail("the JSON number at /reading/value is refused: the number needs more than 131072 digits before the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_small_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_valid_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_attribute("reading", any_attribute().value(1)));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_large_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    Ok(())
}
