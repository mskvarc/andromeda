//! A unit that is no common code refuses only its entity.

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
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Batch Entity Upsert of an entity with `unitCode` KMH and one with `unitCode` km/h answers 207, stores the
/// first with its unit, and reports the second as `BadRequestData` naming `unitCode`
/// Clause: Table 5.2.5-1, 5.6.8.4
#[gtest]
#[tokio::test]
async fn o_bat_res_9002_a_unit_that_is_no_common_code_refuses_only_its_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let batch_entity_error_valid_id = namespace.urn("BatchEntityError", "valid")?;
    let batch_entity_error_type = namespace.entity_type("BatchEntityError")?;
    let batch_entity_error_refused_id = namespace.urn("BatchEntityError", "refused")?;
    let batch_entity_error_valid = Entity::builder()
        .id(batch_entity_error_valid_id.clone())
        .entity_type(batch_entity_error_type.clone())
        .property("speed", Property::builder(1).unit_code(UnitCode::Kmh))
        .build()?;
    let document = UncheckedDocument::from_entity(&batch_entity_error_valid, Lossless::Normalized);
    let batch_entity_error_refused = Entity::builder()
        .id(batch_entity_error_refused_id.clone())
        .entity_type(batch_entity_error_type.clone())
        .build()?;
    // The model refuses it (invalid unit code at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "unitCode": "km/h"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&batch_entity_error_refused, Lossless::Normalized).set_member("speed", &speed);
    let entities = [batch_entity_error_valid, batch_entity_error_refused];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&batch_entity_error_valid_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&batch_entity_error_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'unitCode'")
                .with_detail_containing("not a UN/CEFACT common code")
        )
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&batch_entity_error_valid_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().unit_code(UnitCode::Kmh)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&batch_entity_error_refused_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
