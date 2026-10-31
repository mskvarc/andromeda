//! A batch create refuses every null form.

use andromeda_test_suite::{broker, namespace::Namespace, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Creation of three Entities holding urn:ngsi-ld:null as a member, as a Property value and
/// under a key of a Property value answers 207 with three `BadRequestData` errors and creates none of them
/// Clause: 5.6.7.4, 5.5.4, 5.2.1
#[gtest]
#[tokio::test]
async fn n_bat_cre_9004_a_batch_create_refuses_every_null_form() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_create_first_level_id = namespace.urn("Building", "batch-create-first-level")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_create_value_id = namespace.urn("Building", "batch-create-value")?;
    let building_batch_create_nested_id = namespace.urn("Building", "batch-create-nested")?;
    let building_batch_create_first_level = Entity::builder()
        .id(building_batch_create_first_level_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at temperature), so `temperature` is written as text.
    let temperature: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document = UncheckedDocument::from_entity(&building_batch_create_first_level, Lossless::Normalized).set_member("temperature", &temperature);
    let building_batch_create_value = Entity::builder()
        .id(building_batch_create_value_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at temperature), so `temperature` is written as text.
    let temperature_2: RawJson = r#"{"type": "Property", "value": "urn:ngsi-ld:null"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_batch_create_value, Lossless::Normalized).set_member("temperature", &temperature_2);
    let building_batch_create_nested = Entity::builder()
        .id(building_batch_create_nested_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at address), so `address` is written as text.
    let address: RawJson = r#"{"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_batch_create_nested, Lossless::Normalized).set_member("address", &address);
    let entities = [building_batch_create_first_level, building_batch_create_value, building_batch_create_nested];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2, document_3]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(3)));
    expect_that!(&report.errors, each(batch_error().problem(ErrorType::BadRequestData)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_create_first_level_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_create_value_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_create_nested_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
