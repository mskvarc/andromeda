//! An entity without a type is refused per entity.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, problem::problem, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: a Batch Entity Upsert of two valid new entities and one without a type answers 207 with the two valid ids
/// in success and a `BadRequestData` error for the third, which is not created
/// Clause: 5.6.8.4, 5.6.8.5, 5.2.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0013_an_entity_without_a_type_is_refused_per_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let placeholder_type = namespace.entity_type("Placeholder")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized);
    let building_2 = Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?;
    let document_2 = UncheckedDocument::from_entity(&building_2, Lossless::Normalized);
    let building_3 = Entity::builder().id(building_3_id.clone()).entity_type(placeholder_type).build()?;
    // The model requires `type`, which the request leaves out, so it is written for a placeholder and removed.
    let document_3 = UncheckedDocument::from_entity(&building_3, Lossless::Normalized).remove_member("type");
    let entities = [building, building_2, building_3];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[document, document_2, document_3]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(2)));
    expect_that!(&report.success, contains(eq(&building_id)));
    expect_that!(&report.success, contains(eq(&building_2_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&building_3_id).problem(ErrorType::BadRequestData)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_3_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
