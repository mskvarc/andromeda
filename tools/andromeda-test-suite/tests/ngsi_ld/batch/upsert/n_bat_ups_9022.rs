//! The NGSI-LD Null as a type refuses only its own entity in a batch upsert.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Upsert of a new Entity of type "urn:ngsi-ld:null", of a stored Entity replaced with type
/// `["Vehicle", "urn:ngsi-ld:null"]` and `speed` 60, and of a new Entity of type Vehicle answers 207 with
/// `BadRequestData` for the first two alone; the first is not created, the second still holds `speed` 50 and the
/// third is created
/// Clause: 5.5.4, 5.6.8.4
#[gtest]
#[tokio::test]
async fn n_bat_ups_9022_the_null_as_a_type_refuses_only_its_own_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let null_typed_id = namespace.urn("Vehicle", "upsert-null-typed")?;
    let stored_id = namespace.urn("Vehicle", "upsert-stored")?;
    let typed_id = namespace.urn("Vehicle", "upsert-typed")?;
    let stored = Entity::builder()
        .id(stored_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&stored).send().await?.done()?;
    let null_typed = Entity::builder()
        .id(null_typed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    let replacement = Entity::builder()
        .id(stored_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    let typed = Entity::builder()
        .id(typed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    // The model holds type names, so the Null is written as text.
    let beside: RawJson = format!(r#"["{vehicle_type}", "urn:ngsi-ld:null"]"#).parse()?;
    let documents = [
        UncheckedDocument::from_entity(&null_typed, Lossless::Normalized).set_text("type", "urn:ngsi-ld:null"),
        UncheckedDocument::from_entity(&replacement, Lossless::Normalized).set_member("type", &beside),
        UncheckedDocument::from_entity(&typed, Lossless::Normalized),
    ];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&[null_typed, replacement, typed]).build())?
        .documents(&documents);
    let report: BatchOperationResult = reported(broker.send_unchecked(request).await)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&typed_id)));
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(
        &report.errors,
        unordered_elements_are![
            batch_error().entity(&null_typed_id).problem(ErrorType::BadRequestData),
            batch_error().entity(&stored_id).problem(ErrorType::BadRequestData)
        ]
    );
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    expect_that!(
        &json.with_responses().entities().retrieve(&null_typed_id).send().await,
        answered(StatusCode::NOT_FOUND)
    );
    let retrieved = json.entities().retrieve(&stored_id).send().await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(50)));
    let created = json.entities().retrieve(&typed_id).send().await?;
    expect_that!(created.body(), has_attribute("speed", any_attribute().value(60)));
    Ok(())
}
