//! An update mode upsert creates no entity typed as the NGSI-LD Null.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchUpsert, UpsertMode},
    },
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Upsert in update mode of a new Entity of type `["Vehicle", "urn:ngsi-ld:null"]` and of a new
/// Entity of type Vehicle answers 207 with `BadRequestData` for the first alone, the first is not created and the
/// second is
/// Clause: 5.5.4, 5.6.8.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_bat_ups_9023_an_update_mode_upsert_creates_no_entity_typed_as_the_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let null_typed_id = namespace.urn("Vehicle", "upsert-update-null-typed")?;
    let typed_id = namespace.urn("Vehicle", "upsert-update-typed")?;
    let null_typed = Entity::builder()
        .id(null_typed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    let typed = Entity::builder()
        .id(typed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    // The model holds type names, so the Null is written as text.
    let beside: RawJson = format!(r#"["{vehicle_type}", "urn:ngsi-ld:null"]"#).parse()?;
    let documents = [
        UncheckedDocument::from_entity(&null_typed, Lossless::Normalized).set_member("type", &beside),
        UncheckedDocument::from_entity(&typed, Lossless::Normalized),
    ];
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&[null_typed, typed]).mode(UpsertMode::Update).build())?
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
