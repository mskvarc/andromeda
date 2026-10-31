//! A context nested in an attribute is a per entity bad request data.

use andromeda_test_suite::{
    broker,
    core_context::core_context,
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, ContextDelivery, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Upsert refuses the entity whose attribute holds a nested user @context with `BadRequestData`
/// in the 207 answer and stores nothing of it
/// Clause: 5.5.7, 5.6.8
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_bat_ups_2001_a_context_nested_in_an_attribute_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "@context": {"speed": "https://example.org/speed"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("speed", &speed);
    let entities = [device];
    let request = core_context_body
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[document]);
    let answer = core_context_body.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&device_id).problem(ErrorType::BadRequestData)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
