//! An upsert of numbers beyond the numeric range is refused naming the limit.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Upsert over an existing entity answers 207 with a `BadRequestData` error naming the broken
/// limit for the entity holding a number beyond the numeric range, leaves the existing entity unchanged and upserts
/// the valid new entity
/// Clause: 4.5.1, 5.6.8
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM 009
/// v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others naming the broken limit
#[gtest]
#[tokio::test]
async fn o_ent_cre_2306_an_upsert_of_numbers_beyond_the_numeric_range_is_refused_naming_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("big", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (a number outside the range of a double: 1e-16384), so `tiny` is written as text.
    let tiny: RawJson = r#"{"type": "Property", "value": 1e-16384}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle_2, Lossless::Normalized).set_member("tiny", &tiny);
    let vehicle_2_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("big", 1)
        .build()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_2_2, Lossless::Normalized);
    let entities = [vehicle_2, vehicle_2_2];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&vehicle_2_id)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&vehicle_id).problem(ErrorType::BadRequestData).with_detail("the JSON number at /tiny/value is refused: the number needs more than 16383 digits after the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("tiny"));
    expect_that!(entity, has_attribute("big", any_attribute().value(1)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("big", any_attribute().value(1)));
    Ok(())
}
