//! An embedded context refuses only its entity in every batch.

use andromeda_test_suite::{
    broker,
    core_context::core_context_list,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        ContextDelivery,
        UncheckedDocument,
        operation::{BatchCreate, BatchMerge, BatchUpdate, BatchUpsert},
    },
    model::{Entity, EntityFragment, ErrorType, Lossless, RawJson},
};

/// Pins: Batch Entity Creation, Upsert, Update and Merge of a clean entity beside one with an @context inside an
/// attribute each answer 207 with a `BadRequestData` error against the second entity alone
/// Clause: 5.5.7, 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.6.20.4
#[gtest]
#[tokio::test]
async fn n_bat_res_9009_an_embedded_context_refuses_only_its_entity_in_every_batch() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_create_admitted_id = namespace.urn("Device", "create-admitted")?;
    let device_type = namespace.entity_type("Device")?;
    let device_create_refused_id = namespace.urn("Device", "create-refused")?;
    let device_upsert_admitted_id = namespace.urn("Device", "upsert-admitted")?;
    let device_upsert_refused_id = namespace.urn("Device", "upsert-refused")?;
    let device_update_admitted_id = namespace.urn("Device", "update-admitted")?;
    let device_update_refused_id = namespace.urn("Device", "update-refused")?;
    let device_merge_admitted_id = namespace.urn("Device", "merge-admitted")?;
    let device_merge_refused_id = namespace.urn("Device", "merge-refused")?;
    let device_create_admitted = Entity::builder()
        .id(device_create_admitted_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 1)
        .build()?;
    let document = UncheckedDocument::from_entity(&device_create_admitted, Lossless::Normalized);
    let device_create_refused = Entity::builder()
        .id(device_create_refused_id.clone())
        .entity_type(device_type.clone())
        .build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 2, "@context": {}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&device_create_refused, Lossless::Normalized).set_member("speed", &speed);
    let entities = [device_create_admitted, device_create_refused];
    let request = core_context_list_body
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = core_context_list_body.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&device_create_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("@context")
        )
    );
    expect_that!(&report.errors, not(contains(batch_error().entity(&device_create_admitted_id))));
    let device_upsert_admitted = Entity::builder()
        .id(device_upsert_admitted_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 1)
        .build()?;
    let document_3 = UncheckedDocument::from_entity(&device_upsert_admitted, Lossless::Normalized);
    let device_upsert_refused = Entity::builder()
        .id(device_upsert_refused_id.clone())
        .entity_type(device_type.clone())
        .build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "value": 2, "@context": {}}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&device_upsert_refused, Lossless::Normalized).set_member("speed", &speed_2);
    let entities_2 = [device_upsert_admitted, device_upsert_refused];
    let request_2 = core_context_list_body
        .unchecked(&BatchUpsert::builder().entities(&entities_2).build())?
        .documents(&[document_3, document_4]);
    let answer_2 = core_context_list_body.send_unchecked(request_2).await;
    let report_2: BatchOperationResult = reported(answer_2)?;
    expect_that!(&report_2.errors, len(eq(1)));
    expect_that!(
        &report_2.errors,
        contains(
            batch_error()
                .entity(&device_upsert_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("@context")
        )
    );
    expect_that!(&report_2.errors, not(contains(batch_error().entity(&device_upsert_admitted_id))));
    let entities_3 = [
        Entity::builder()
            .id(device_update_admitted_id.clone())
            .entity_type(device_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(device_update_refused_id.clone())
            .entity_type(device_type.clone())
            .property("speed", 1)
            .build()?,
    ];
    core_context_list_body.entities().batch_create(&entities_3).send().await?.done()?;
    let device_update_admitted_fragment = EntityFragment::for_entity(device_update_admitted_id.clone(), device_type.clone())
        .property("speed", 1)
        .build()?;
    let document_5 = UncheckedDocument::try_from(device_update_admitted_fragment.render(Lossless::Normalized))?;
    let device_update_refused_fragment = EntityFragment::for_entity(device_update_refused_id.clone(), device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed_3: RawJson = r#"{"type": "Property", "value": 2, "@context": {}}"#.parse()?;
    let document_6 = UncheckedDocument::try_from(device_update_refused_fragment.render(Lossless::Normalized))?.set_member("speed", &speed_3);
    let fragments = [device_update_admitted_fragment, device_update_refused_fragment];
    let request_3 = core_context_list_body
        .unchecked(&BatchUpdate::builder().fragments(&fragments).build())?
        .documents(&[document_5, document_6]);
    let answer_3 = core_context_list_body.send_unchecked(request_3).await;
    let report_3: BatchOperationResult = reported(answer_3)?;
    expect_that!(&report_3.errors, len(eq(1)));
    expect_that!(
        &report_3.errors,
        contains(
            batch_error()
                .entity(&device_update_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("@context")
        )
    );
    expect_that!(&report_3.errors, not(contains(batch_error().entity(&device_update_admitted_id))));
    let entities_4 = [
        Entity::builder()
            .id(device_merge_admitted_id.clone())
            .entity_type(device_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(device_merge_refused_id.clone())
            .entity_type(device_type.clone())
            .property("speed", 1)
            .build()?,
    ];
    core_context_list_body.entities().batch_create(&entities_4).send().await?.done()?;
    let device_merge_admitted_fragment = EntityFragment::for_entity(device_merge_admitted_id.clone(), device_type.clone())
        .property("speed", 1)
        .build()?;
    let document_7 = UncheckedDocument::try_from(device_merge_admitted_fragment.render(Lossless::Normalized))?;
    let device_merge_refused_fragment = EntityFragment::for_entity(device_merge_refused_id.clone(), device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed_4: RawJson = r#"{"type": "Property", "value": 2, "@context": {}}"#.parse()?;
    let document_8 = UncheckedDocument::try_from(device_merge_refused_fragment.render(Lossless::Normalized))?.set_member("speed", &speed_4);
    let fragments_2 = [device_merge_admitted_fragment, device_merge_refused_fragment];
    let request_4 = core_context_list_body
        .unchecked(&BatchMerge::builder().fragments(&fragments_2).build())?
        .documents(&[document_7, document_8]);
    let answer_4 = core_context_list_body.send_unchecked(request_4).await;
    let report_4: BatchOperationResult = reported(answer_4)?;
    expect_that!(&report_4.errors, len(eq(1)));
    expect_that!(
        &report_4.errors,
        contains(
            batch_error()
                .entity(&device_merge_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("@context")
        )
    );
    expect_that!(&report_4.errors, not(contains(batch_error().entity(&device_merge_admitted_id))));
    Ok(())
}
