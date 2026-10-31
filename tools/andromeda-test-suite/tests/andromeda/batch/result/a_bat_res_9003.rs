//! Create reports each refused entity and writes the rest.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Creation of a valid entity, one whose id is past the 1024-byte identifier bound and one with
/// a null value answers 207, creates the valid one, and reports the other two against their own ids, the first
/// naming the id and the bound
/// Clause: 5.6.7.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_bat_res_9003_create_reports_each_refused_entity_and_writes_the_rest() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let batch_entity_error_valid_id = namespace.urn("BatchEntityError", "valid")?;
    let batch_entity_error_type = namespace.entity_type("BatchEntityError")?;
    let batch_entity_error_create_aaaaaaaaaaaaaaaaaaa_id = namespace.urn("BatchEntityError", format!("create-{}", "a".repeat(987)).as_str())?;
    let batch_entity_error_null_value_id = namespace.urn("BatchEntityError", "null-value")?;
    // The identifier is past the 1024-byte bound by however long the run makes {{ns}}, so its length is not asserted.
    let batch_entity_error_valid = Entity::builder()
        .id(batch_entity_error_valid_id.clone())
        .entity_type(batch_entity_error_type.clone())
        .property("speed", 1)
        .build()?;
    let document = UncheckedDocument::from_entity(&batch_entity_error_valid, Lossless::Normalized);
    let batch_entity_error_create_aaaaaaaaaaaaaaaaaaa = Entity::builder()
        .id(batch_entity_error_create_aaaaaaaaaaaaaaaaaaa_id.clone())
        .entity_type(batch_entity_error_type.clone())
        .property("speed", 2)
        .build()?;
    let document_2 = UncheckedDocument::from_entity(&batch_entity_error_create_aaaaaaaaaaaaaaaaaaa, Lossless::Normalized);
    let batch_entity_error_null_value = Entity::builder()
        .id(batch_entity_error_null_value_id.clone())
        .entity_type(batch_entity_error_type.clone())
        .build()?;
    // The model refuses it (invalid value at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&batch_entity_error_null_value, Lossless::Normalized).set_member("speed", &speed);
    let entities = [
        batch_entity_error_valid,
        batch_entity_error_create_aaaaaaaaaaaaaaaaaaa,
        batch_entity_error_null_value,
    ];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
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
                .entity(&batch_entity_error_create_aaaaaaaaaaaaaaaaaaa_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'id'")
                .with_detail_containing("1024 bytes")
        )
    );
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&batch_entity_error_null_value_id).problem(ErrorType::BadRequestData))
    );
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&batch_entity_error_valid_id)
        .send()
        .await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&batch_entity_error_null_value_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
