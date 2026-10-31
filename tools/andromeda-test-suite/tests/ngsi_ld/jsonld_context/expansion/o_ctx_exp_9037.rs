//! Every batch operation refuses the whole request.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, EntityFragment, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Batch Entity Create, Upsert, Update and Merge of an Entity whose @context defines gear with a scoped
/// @context each answer 400 `BadRequestData` for the request rather than 207
/// Clause: 5.5.7, 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.6.20.4
/// Recorded: 5.5.7 makes the user @context of the request the subject and the batch clauses report entity failures
/// per entity; Andromeda treats a scoped @context as a fault of the request and refuses it whole
#[gtest]
#[tokio::test]
async fn o_ctx_exp_9037_every_batch_operation_refuses_the_whole_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_batch_create_id = namespace.urn("Device", "batch-create")?;
    let device_type = namespace.entity_type("Device")?;
    let device_batch_upsert_id = namespace.urn("Device", "batch-upsert")?;
    let device_batch_update_id = namespace.urn("Device", "batch-update")?;
    let device_batch_merge_id = namespace.urn("Device", "batch-merge")?;
    let entities = [Entity::builder()
        .id(device_batch_create_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?];
    let answer = body_context.with_responses().entities().batch_create(&entities).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let entities_2 = [Entity::builder()
        .id(device_batch_upsert_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?];
    let answer_2 = body_context.with_responses().entities().batch_upsert(&entities_2).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragments = [EntityFragment::for_entity(device_batch_update_id, device_type.clone())
        .property("gear", 3)
        .build()?];
    let answer_3 = body_context.with_responses().entities().batch_update(&fragments).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragments_2 = [EntityFragment::for_entity(device_batch_merge_id, device_type.clone())
        .property("gear", 3)
        .build()?];
    let answer_4 = body_context.with_responses().entities().batch_merge(&fragments_2).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_batch_create_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_batch_upsert_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    Ok(())
}
