//! An inline user context declaring a scoped context is bad request data.

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
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Create Entity whose body @context defines gear with a scoped @context answers 400 `BadRequestData` and
/// creates nothing
/// Clause: 4.4, 5.5.7, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9030_an_inline_user_context_declaring_a_scoped_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_inline_id = namespace.urn("Device", "inline")?;
    let device_type = namespace.entity_type("Device")?;
    let device_inline = Entity::builder()
        .id(device_inline_id.clone())
        .entity_type(device_type)
        .property("gear", 3)
        .build()?;
    let answer = body_context.with_responses().entities().create(&device_inline).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_inline_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
