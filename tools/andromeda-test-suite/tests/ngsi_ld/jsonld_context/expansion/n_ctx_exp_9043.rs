//! A subscription under a scoped body context is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose body @context defines gear with a scoped @context answers 400 `BadRequestData`
/// and stores nothing, and with a plain one 201
/// Clause: 5.5.7, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9043_a_subscription_under_a_scoped_body_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_scoped_id = namespace.subscription_id("scoped")?;
    let device_type = namespace.entity_type("Device")?;
    let body_context_2 = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            ContextEntry::Inline(format!(r#"{{"gear": "https://example.org/{namespace}/gear"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_plain_id = namespace.subscription_id("plain")?;
    let subscription_scoped = Subscription::builder()
        .id(subscription_scoped_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let answer = body_context.with_responses().subscriptions().create(&subscription_scoped).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_scoped_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_plain = Subscription::builder()
        .id(subscription_plain_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    body_context_2.subscriptions().create(&subscription_plain).send().await?;
    broker.subscriptions().delete(&subscription_plain_id).send().await?;
    Ok(())
}
