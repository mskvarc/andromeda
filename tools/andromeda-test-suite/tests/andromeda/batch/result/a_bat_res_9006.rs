//! A batch in which every entity is refused is a 207.

use andromeda_test_suite::{broker, namespace::Namespace, partial_outcome::reported, report_matcher::batch_error};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Batch Entity Creation of an entity whose id is past the identifier bound and one with a null value answers
/// 207 with no success and two errors
/// Clause: Table 6.14.3.1-1, 5.6.7.4
#[gtest]
#[tokio::test]
async fn a_bat_res_9006_a_batch_in_which_every_entity_is_refused_is_a_207() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let batch_entity_error_none_aaaaaaaaaaaaaaaaaaaaa_id = namespace.urn("BatchEntityError", format!("none-{}", "a".repeat(989)).as_str())?;
    let batch_entity_error_type = namespace.entity_type("BatchEntityError")?;
    let batch_entity_error_null_value_id = namespace.urn("BatchEntityError", "null-value")?;
    // The identifier is past the 1024-byte bound by however long the run makes {{ns}}, so its length is not asserted.
    let batch_entity_error_none_aaaaaaaaaaaaaaaaaaaaa = Entity::builder()
        .id(batch_entity_error_none_aaaaaaaaaaaaaaaaaaaaa_id.clone())
        .entity_type(batch_entity_error_type.clone())
        .property("speed", 1)
        .build()?;
    let document = UncheckedDocument::from_entity(&batch_entity_error_none_aaaaaaaaaaaaaaaaaaaaa, Lossless::Normalized);
    let batch_entity_error_null_value = Entity::builder()
        .id(batch_entity_error_null_value_id.clone())
        .entity_type(batch_entity_error_type.clone())
        .build()?;
    // The model refuses it (invalid value at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&batch_entity_error_null_value, Lossless::Normalized).set_member("speed", &speed);
    let entities = [batch_entity_error_none_aaaaaaaaaaaaaaaaaaaaa, batch_entity_error_null_value];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&batch_entity_error_none_aaaaaaaaaaaaaaaaaaaaa_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'id'")
                .with_detail_containing("1024 bytes")
        )
    );
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&batch_entity_error_null_value_id).problem(ErrorType::BadRequestData))
    );
    Ok(())
}
