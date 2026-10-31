//! A relative context reference in a subscription or registration body resolves as create does.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    hosted_context::HostedContext,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relative_context::with_context_text,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{CreateSubscription, RegisterContextSource},
    },
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use serde_json::json;

/// Pins: the Location of a stored @context, a reference relative to the broker's origin, resolves as the body
/// @context of Create Subscription, Create Context Source Registration and Create Context Source Registration
/// Subscription, as it does for Create Entity
/// Clause: 5.13.2, 6.3.5, 5.8.1, 5.9.2, 5.11.2
/// Recorded: the specification does not say how a relative context reference resolves; the defect is that one
/// broker operation resolves its own Location and another refuses it
/// Vocabulary: none (the test is about @context resolution itself)
#[gtest]
#[tokio::test]
async fn o_ctx_exp_2002_a_relative_context_reference_in_a_subscription_or_registration_body_resolves_as_create_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "speed": "https://test.andromeda/{namespace}/speed"}}"#)
            .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    expect_that!(ctx.location.as_str(), starts_with("/ngsi-ld/v1/jsonldContexts/"));
    let body_context = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let reference = json!(ctx.location);
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The client writes no relative reference into a body, so each body's @context is replaced by the Location.
    let request = with_context_text(body_context.unchecked(&CreateSubscription { subscription: &subscription })?, &reference)?;
    let created = body_context.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let registration = CSourceRegistration::builder(source.endpoint("sources")?.as_str())
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(namespace.urn("Vehicle", 1)?)])
            .property_names(["speed"])
            .build()?])
        .build()?;
    let request_2 = with_context_text(body_context.unchecked(&RegisterContextSource { registration: &registration })?, &reference)?;
    let registered = body_context.send_unchecked(request_2).await;
    expect_that!(&registered, answered(StatusCode::CREATED));
    let registration_subscription = Subscription::registration_builder()
        .id(namespace.subscription_id(2)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let request_3 = with_context_text(
        body_context.unchecked(&CreateSubscription {
            subscription: &registration_subscription,
        })?,
        &reference,
    )?;
    let registration_subscribed = body_context.send_unchecked(request_3).await;
    expect_that!(&registration_subscribed, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        retrieved.entities().first().map(|selector| selector.selector_type().to_string()),
        some(eq(&format!("https://test.andromeda/{namespace}/Vehicle")))
    );
    Ok(())
}
