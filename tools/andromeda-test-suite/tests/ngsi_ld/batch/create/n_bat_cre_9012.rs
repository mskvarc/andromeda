//! The NGSI-LD Null as a type refuses only its own entity in a batch create.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Batch Entity Create of one Entity of type "urn:ngsi-ld:null" and one of type Vehicle answers 207 with
/// `BadRequestData` for the first alone, the first is not created and the second is
/// Clause: 5.5.4, 5.6.7.4
#[gtest]
#[tokio::test]
async fn n_bat_cre_9012_the_null_as_a_type_refuses_only_its_own_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let null_typed_id = namespace.urn("Vehicle", "batch-null-typed")?;
    let typed_id = namespace.urn("Vehicle", "batch-typed")?;
    let null_typed = Entity::builder()
        .id(null_typed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    let typed = Entity::builder().id(typed_id.clone()).entity_type(vehicle_type).property("speed", 50).build()?;
    // The model holds a type name, so the Null is written as text.
    let documents = [
        UncheckedDocument::from_entity(&null_typed, Lossless::Normalized).set_text("type", "urn:ngsi-ld:null"),
        UncheckedDocument::from_entity(&typed, Lossless::Normalized),
    ];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&[null_typed, typed]).build())?
        .documents(&documents);
    let report: BatchOperationResult = reported(broker.send_unchecked(request).await)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&typed_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&null_typed_id).problem(ErrorType::BadRequestData)));
    let json = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    expect_that!(&json.entities().retrieve(&null_typed_id).send().await, answered(StatusCode::NOT_FOUND));
    expect_that!(&json.entities().retrieve(&typed_id).send().await, answered(StatusCode::OK));
    Ok(())
}
