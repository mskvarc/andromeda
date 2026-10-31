//! A context nested in entities or notification is bad request data.

use andromeda_test_suite::{broker, core_context::core_context, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Context Source Registration Subscription refuses a user @context nested in an entities member and
/// in the notification, and leaves the subscription unchanged
/// Clause: 5.5.7, 5.11.3
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_css_upd_2001_a_context_nested_in_entities_or_notification_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .description("before")
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::registration_builder().description("after").build()?;
    // The model refuses it (invalid entity selector at entities.0), so `entities` is written as text.
    let entities: RawJson = format!(r#"[{{"type": "Device_{namespace}", "@context": {{}}}}]"#).parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("entities", &entities);
    let request = core_context_body
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = core_context_body.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = SubscriptionFragment::registration_builder().description("after").build()?;
    // The model refuses it (member `@context` is not allowed here at notification), so `notification` is written as text.
    let endpoint = sink.endpoint("s1")?;
    let notification: RawJson = format!(r#"{{"endpoint": {{"uri": "{endpoint}", "accept": "application/json"}}, "@context": {{}}}}"#).parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render())?.set_member("notification", &notification);
    let request_2 = core_context_body
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment_2,
        })?
        .document(&document_2);
    let answer_2 = core_context_body.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.description(), some(eq("before")));
    Ok(())
}
