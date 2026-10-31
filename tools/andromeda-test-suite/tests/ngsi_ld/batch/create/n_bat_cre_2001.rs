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
    http::{BatchOperationResult, ContextDelivery, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Creation refuses the entity whose attribute holds a nested user @context with
/// `BadRequestData` in the 207 answer and creates nothing of it, while the clean entity beside it is created
/// Clause: 5.5.7, 5.6.7
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_bat_cre_2001_a_context_nested_in_an_attribute_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "@context": {"speed": "https://example.org/speed"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("speed", &speed);
    let device_2 = Entity::builder()
        .id(device_2_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 2)
        .build()?;
    let document_2 = UncheckedDocument::from_entity(&device_2, Lossless::Normalized);
    let entities = [device, device_2];
    let request = core_context_body
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = core_context_body.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, nth(0, eq(&device_2_id)));
    expect_that!(&report.errors, contains(batch_error().entity(&device_id).problem(ErrorType::BadRequestData)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_2_id)
        .send()
        .await?;
    Ok(())
}
