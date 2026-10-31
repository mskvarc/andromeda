//! A context nested in an attribute is a per entity bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_context,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, ContextDelivery, UncheckedDocument, headers::Accept, operation::BatchUpdate},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Update refuses the entity whose attribute holds a nested user @context with `BadRequestData`
/// in the 207 answer and leaves the stored value alone
/// Clause: 5.5.7, 5.6.9
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_bat_upd_2001_a_context_nested_in_an_attribute_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    let device_fragment = EntityFragment::for_entity(device_id.clone(), device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 9, "@context": {"speed": "https://example.org/speed"}}"#.parse()?;
    let document = UncheckedDocument::try_from(device_fragment.render(Lossless::Normalized))?.set_member("speed", &speed);
    let fragments = [device_fragment];
    let request = core_context_body
        .unchecked(&BatchUpdate::builder().fragments(&fragments).build())?
        .documents(&[document]);
    let answer = core_context_body.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&device_id).problem(ErrorType::BadRequestData)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
