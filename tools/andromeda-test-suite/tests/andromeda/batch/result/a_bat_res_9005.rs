//! Update and merge apply the valid entity and leave the refused one.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchMerge, BatchUpdate, RepresentationSpelling},
    },
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Update and then Batch Entity Merge of a valid entity, one whose id is past the identifier
/// bound and an existing one with a null value answer 207, apply the valid one, report the other two, and leave the
/// existing refused entity as it was
/// Clause: 5.6.9.4, 5.6.20.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_bat_res_9005_update_and_merge_apply_the_valid_entity_and_leave_the_refused_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let batch_entity_error_valid_id = namespace.urn("BatchEntityError", "valid")?;
    let batch_entity_error_type = namespace.entity_type("BatchEntityError")?;
    let batch_entity_error_refused_id = namespace.urn("BatchEntityError", "refused")?;
    let batch_entity_error_update_aaaaaaaaaaaaaaaaaaa_id = namespace.urn("BatchEntityError", format!("update-{}", "a".repeat(987)).as_str())?;
    let entities = [
        Entity::builder()
            .id(batch_entity_error_valid_id.clone())
            .entity_type(batch_entity_error_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(batch_entity_error_refused_id.clone())
            .entity_type(batch_entity_error_type.clone())
            .property("speed", 1)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The identifier is past the 1024-byte bound by however long the run makes {{ns}}, so its length is not asserted.
    let batch_entity_error_valid_fragment = EntityFragment::for_entity(batch_entity_error_valid_id.clone(), batch_entity_error_type.clone())
        .property("speed", 2)
        .build()?;
    let document = UncheckedDocument::try_from(batch_entity_error_valid_fragment.render(Lossless::Normalized))?;
    let batch_entity_error_update_aaaaaaaaaaaaa_fragment =
        EntityFragment::for_entity(batch_entity_error_update_aaaaaaaaaaaaaaaaaaa_id.clone(), batch_entity_error_type.clone())
            .property("speed", 2)
            .build()?;
    let document_2 = UncheckedDocument::try_from(batch_entity_error_update_aaaaaaaaaaaaa_fragment.render(Lossless::Normalized))?;
    let batch_entity_error_refused_fragment = EntityFragment::for_entity(batch_entity_error_refused_id.clone(), batch_entity_error_type.clone()).build()?;
    // The model refuses it (invalid value at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(batch_entity_error_refused_fragment.render(Lossless::Normalized))?.set_member("speed", &speed);
    let fragments = [
        batch_entity_error_valid_fragment,
        batch_entity_error_update_aaaaaaaaaaaaa_fragment,
        batch_entity_error_refused_fragment,
    ];
    let request = broker
        .unchecked(&BatchUpdate::builder().fragments(&fragments).build())?
        .documents(&[document, document_2, document_3]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&batch_entity_error_valid_id)));
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&batch_entity_error_update_aaaaaaaaaaaaaaaaaaa_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'id'")
                .with_detail_containing("1024 bytes")
        )
    );
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&batch_entity_error_refused_id).problem(ErrorType::BadRequestData))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&batch_entity_error_valid_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_value("speed", 2));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&batch_entity_error_refused_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value("speed", 1));
    let batch_entity_error_valid_fragment_2 = EntityFragment::for_entity(batch_entity_error_valid_id.clone(), batch_entity_error_type.clone())
        .property("speed", 3)
        .build()?;
    let document_4 = UncheckedDocument::try_from(batch_entity_error_valid_fragment_2.render(Lossless::Normalized))?;
    let batch_entity_error_update_aaaaaaaaaaaaa_fragment_2 =
        EntityFragment::for_entity(batch_entity_error_update_aaaaaaaaaaaaaaaaaaa_id.clone(), batch_entity_error_type.clone())
            .property("speed", 3)
            .build()?;
    let document_5 = UncheckedDocument::try_from(batch_entity_error_update_aaaaaaaaaaaaa_fragment_2.render(Lossless::Normalized))?;
    let batch_entity_error_refused_fragment_2 = EntityFragment::for_entity(batch_entity_error_refused_id.clone(), batch_entity_error_type.clone()).build()?;
    // The model refuses it (invalid value at speed), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document_6 = UncheckedDocument::try_from(batch_entity_error_refused_fragment_2.render(Lossless::Normalized))?.set_member("speed", &speed_2);
    let fragments_2 = [
        batch_entity_error_valid_fragment_2,
        batch_entity_error_update_aaaaaaaaaaaaa_fragment_2,
        batch_entity_error_refused_fragment_2,
    ];
    let request_2 = broker
        .unchecked(&BatchMerge::builder().fragments(&fragments_2).build())?
        .documents(&[document_4, document_5, document_6]);
    let answer_2 = broker.send_unchecked(request_2).await;
    let report_2: BatchOperationResult = reported(answer_2)?;
    expect_that!(&report_2.success, len(eq(1)));
    expect_that!(&report_2.success, nth(0, eq(&batch_entity_error_valid_id)));
    expect_that!(&report_2.errors, len(eq(2)));
    expect_that!(
        &report_2.errors,
        contains(
            batch_error()
                .entity(&batch_entity_error_update_aaaaaaaaaaaaaaaaaaa_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'id'")
                .with_detail_containing("1024 bytes")
        )
    );
    expect_that!(
        &report_2.errors,
        contains(batch_error().entity(&batch_entity_error_refused_id).problem(ErrorType::BadRequestData))
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&batch_entity_error_valid_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_value("speed", 3));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&batch_entity_error_refused_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4, has_value("speed", 1));
    Ok(())
}
