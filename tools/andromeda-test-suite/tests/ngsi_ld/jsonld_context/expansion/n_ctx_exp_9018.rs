//! Replace and merge entity refuse an embedded context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_context_list,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Replace Entity with an @context inside a Property value object and Merge Entity with an @context inside an
/// attribute answer 400 `BadRequestData`, and the entity keeps its value
/// Clause: 5.5.7, 5.6.17.4, 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9018_replace_and_merge_entity_refuse_an_embedded_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 1)
        .build()?;
    core_context_list_body.entities().create(&device).send().await?.done()?;
    let device_replacement = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property(
            "speed",
            object([
                (
                    "@context",
                    object([("reading", Value::from(format!("https://example.org/{namespace}/reading")))]),
                ),
                ("reading", Value::from(9)),
            ]),
        )
        .build()?;
    let answer = core_context_list_body.with_responses().entities().replace(&device_replacement).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 4, "@context": {}}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = core_context_list_body
        .unchecked(&MergeEntity::builder().id(&device_id).fragment(&fragment).build())?
        .document(&document);
    let answer_2 = core_context_list_body.send_unchecked(request).await;
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
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
