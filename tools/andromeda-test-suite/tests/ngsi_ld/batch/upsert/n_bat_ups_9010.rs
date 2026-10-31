//! An update mode upsert refuses the null at any depth on create.

use andromeda_test_suite::{broker, namespace::Namespace, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchUpsert, UpsertMode},
    },
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Upsert in update mode of six new Entities holding urn:ngsi-ld:null as a member, a `unitCode`,
/// an `observedAt`, a sub-attribute, a `datasetId` instance value or the scope answers 207 with six
/// `BadRequestData` errors and creates none of them
/// Clause: 5.6.8.4, 5.6.1, 5.2.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_9010_an_update_mode_upsert_refuses_the_null_at_any_depth_on_create() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_upsert_create_whole_member_id = namespace.urn("Building", "upsert-create-whole-member")?;
    let building_type = namespace.entity_type("Building")?;
    let building_upsert_create_unit_code_id = namespace.urn("Building", "upsert-create-unit-code")?;
    let building_upsert_create_observed_at_id = namespace.urn("Building", "upsert-create-observed-at")?;
    let building_upsert_create_sub_attribute_id = namespace.urn("Building", "upsert-create-sub-attribute")?;
    let building_upsert_create_dataset_instance_id = namespace.urn("Building", "upsert-create-dataset-instance")?;
    let building_upsert_create_scope_id = namespace.urn("Building", "upsert-create-scope")?;
    let building_upsert_create_whole_member = Entity::builder()
        .id(building_upsert_create_whole_member_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at speed), so `speed` is written as text.
    let speed: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document = UncheckedDocument::from_entity(&building_upsert_create_whole_member, Lossless::Normalized).set_member("speed", &speed);
    let building_upsert_create_unit_code = Entity::builder()
        .id(building_upsert_create_unit_code_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (invalid unit code at speed), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "value": 5, "unitCode": "urn:ngsi-ld:null"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_upsert_create_unit_code, Lossless::Normalized).set_member("speed", &speed_2);
    let building_upsert_create_observed_at = Entity::builder()
        .id(building_upsert_create_observed_at_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed_3: RawJson = r#"{"type": "Property", "value": 5, "observedAt": "urn:ngsi-ld:null"}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_upsert_create_observed_at, Lossless::Normalized).set_member("speed", &speed_3);
    let building_upsert_create_sub_attribute = Entity::builder()
        .id(building_upsert_create_sub_attribute_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at speed.accuracy), so `speed` is written as text.
    let speed_4: RawJson = r#"{"type": "Property", "value": 5, "accuracy": "urn:ngsi-ld:null"}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&building_upsert_create_sub_attribute, Lossless::Normalized).set_member("speed", &speed_4);
    let building_upsert_create_dataset_instance = Entity::builder()
        .id(building_upsert_create_dataset_instance_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at speed), so `speed` is written as text.
    let speed_5: RawJson = format!(
        r#"[{{"type": "Property", "value": "urn:ngsi-ld:null", "datasetId": "urn:ngsi-ld:Dataset:{namespace}-A"}}, {{"type": "Property", "value": 5}}]"#
    )
    .parse()?;
    let document_5 = UncheckedDocument::from_entity(&building_upsert_create_dataset_instance, Lossless::Normalized).set_member("speed", &speed_5);
    let building_upsert_create_scope = Entity::builder()
        .id(building_upsert_create_scope_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (invalid scope), so `scope` is written as text.
    let scope: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document_6 = UncheckedDocument::from_entity(&building_upsert_create_scope, Lossless::Normalized).set_member("scope", &scope);
    let entities = [
        building_upsert_create_whole_member,
        building_upsert_create_unit_code,
        building_upsert_create_observed_at,
        building_upsert_create_sub_attribute,
        building_upsert_create_dataset_instance,
        building_upsert_create_scope,
    ];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&entities).mode(UpsertMode::Update).build())?
        .documents(&[document, document_2, document_3, document_4, document_5, document_6]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(6)));
    expect_that!(&report.errors, each(batch_error().problem(ErrorType::BadRequestData)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_create_whole_member_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_create_unit_code_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_create_observed_at_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_create_sub_attribute_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_create_dataset_instance_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let answer_7 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_create_scope_id)
        .send()
        .await;
    expect_that!(&answer_7, answered(StatusCode::NOT_FOUND));
    Ok(())
}
