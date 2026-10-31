//! Append and update attributes refuse an embedded context.

use andromeda_test_suite::{broker, core_context::core_context_list, entity_matcher::lacks_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        UncheckedDocument,
        headers::Accept,
        operation::{AppendAttributes, UpdateAttributes},
    },
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Append Attributes and Update Attributes with an @context inside the appended attribute answer 400
/// `BadRequestData`, and the entity gains no attribute
/// Clause: 5.5.7, 5.6.2.4, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9016_append_and_update_attributes_refuse_an_embedded_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 1).build()?;
    core_context_list_body.entities().create(&device).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (member `@context` is not allowed here at gear), so `gear` is written as text.
    let gear: RawJson = format!(r#"{{"type": "Property", "value": 3, "@context": {{"gear": "https://example.org/{namespace}/gear"}}}}"#).parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("gear", &gear);
    let request = core_context_list_body
        .unchecked(&AppendAttributes::builder().id(&device_id).fragment(&fragment).build())?
        .document(&document);
    let answer = core_context_list_body.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let fragment_2 = EntityFragment::builder().build()?;
    // The model refuses it (member `@context` is not allowed here at gear), so `gear` is written as text.
    let gear_2: RawJson = format!(r#"{{"type": "Property", "value": 3, "@context": {{"gear": "https://example.org/{namespace}/gear"}}}}"#).parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render(Lossless::Normalized))?.set_member("gear", &gear_2);
    let request_2 = core_context_list_body
        .unchecked(&UpdateAttributes::builder().id(&device_id).fragment(&fragment_2).build())?
        .document(&document_2);
    let answer_2 = core_context_list_body.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("gear"));
    Ok(())
}
