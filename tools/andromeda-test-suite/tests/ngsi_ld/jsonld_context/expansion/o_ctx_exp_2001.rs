//! A relative context reference in a batch body resolves as create does.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    relative_context::with_context_text,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{AppendAttributes, BatchCreate, BatchUpdate, BatchUpsert, CreateEntity},
    },
    model::{Context, ContextEntry, Entity, EntityFragment, InlineContext, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: the Location of a stored @context, a reference relative to the broker's origin, resolves as the body
/// @context of Batch Entity Create, Upsert and Update, and of Append Entity Attributes, as it does for Create
/// Entity
/// Clause: 5.13.2, 6.3.5, 5.6.7
/// Recorded: the specification does not say how a relative context reference resolves; the defect is that one
/// broker operation resolves its own Location and another refuses it
/// Vocabulary: none (the test is about @context resolution itself)
#[gtest]
#[tokio::test]
async fn o_ctx_exp_2001_a_relative_context_reference_in_a_batch_body_resolves_as_create_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "speed": "https://test.andromeda/{namespace}/speed", "gear": "https://test.andromeda/{namespace}/gear"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    expect_that!(ctx.location.as_str(), starts_with("/ngsi-ld/v1/jsonldContexts/"));
    let body_context = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let reference = json!(ctx.location);
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 0)?;
    let vehicle_1_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .build()?;
    // The client writes no relative reference into a body, so each body's @context is replaced by the Location.
    let request = with_context_text(body_context.unchecked(&CreateEntity::builder().entity(&vehicle).build())?, &reference)?;
    let created = body_context.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let vehicle_1 = [Entity::builder()
        .id(vehicle_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 41)
        .build()?];
    let request_2 = with_context_text(body_context.unchecked(&BatchCreate::builder().entities(&vehicle_1).build())?, &reference)?;
    let batch_created = body_context.send_unchecked(request_2).await;
    expect_that!(&batch_created, answered(StatusCode::CREATED));
    let vehicle_2 = [Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .build()?];
    let request_3 = with_context_text(body_context.unchecked(&BatchUpsert::builder().entities(&vehicle_2).build())?, &reference)?;
    let upserted = body_context.send_unchecked(request_3).await;
    expect_that!(&upserted, answered(StatusCode::CREATED));
    let update = [EntityFragment::for_entity(vehicle_1_id.clone(), vehicle_type.clone())
        .property("speed", 51)
        .build()?];
    let request_4 = with_context_text(body_context.unchecked(&BatchUpdate::builder().fragments(&update).build())?, &reference)?;
    let updated = body_context.send_unchecked(request_4).await;
    expect_that!(&updated, answered(StatusCode::NO_CONTENT));
    let appended = EntityFragment::builder().property("gear", 3).build()?;
    let request_5 = with_context_text(
        body_context.unchecked(&AppendAttributes::builder().id(&vehicle_id).fragment(&appended).build())?,
        &reference,
    )?;
    let append_answer = body_context.send_unchecked(request_5).await;
    expect_that!(&append_answer, answered(StatusCode::NO_CONTENT));
    for (id, name, value) in [(&vehicle_1_id, "speed", 51), (&vehicle_2_id, "speed", 42), (&vehicle_id, "gear", 3)] {
        let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).entities().retrieve(id).send().await?;
        expect_that!(
            retrieved.body(),
            has_attribute(&format!("https://test.andromeda/{namespace}/{name}"), any_attribute().value(value))
        );
    }
    Ok(())
}
