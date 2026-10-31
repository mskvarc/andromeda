//! A replace mode upsert refuses the null.

use andromeda_test_suite::{broker, namespace::Namespace, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Upsert in replace mode of an Entity whose Property value is urn:ngsi-ld:null answers 207 with
/// one `BadRequestData` error and creates nothing
/// Clause: 5.6.8.4, 5.6.18.4, 5.2.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_9011_a_replace_mode_upsert_refuses_the_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_upsert_replace_null_id = namespace.urn("Building", "upsert-replace-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_upsert_replace_null = Entity::builder()
        .id(building_upsert_replace_null_id.clone())
        .entity_type(building_type)
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at temperature), so `temperature` is written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": "urn:ngsi-ld:null"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_upsert_replace_null, Lossless::Normalized).set_member("temperature", &temperature);
    let entities = [building_upsert_replace_null];
    let request = broker.unchecked(&BatchUpsert::builder().entities(&entities).build())?.documents(&[document]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, each(batch_error().problem(ErrorType::BadRequestData)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_upsert_replace_null_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
