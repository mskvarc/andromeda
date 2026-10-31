//! Expand values decides whether a subscription is notified.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, operation::CreateSubscription},
    model::{Context, Entity, EntityFragment, Lossless, NgsiLdMediaType, RawJson},
    query::{EntitySelector, builders::SelectorTypeBuilder, q},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Of two Subscriptions with q status=="active" over an Entity whose status is the IRI the @context gives
/// active, only the one with `expandValues` status is notified of a change; once status holds the string active,
/// the other is
/// Clause: 4.9, Table 5.2.12-1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9016_expand_values_decides_whether_a_subscription_is_notified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![contexts.reference("contexts/states.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_values_id = namespace.urn("Device", "values")?;
    let device_type = namespace.entity_type("Device")?;
    let subscription_coerced_id = namespace.subscription_id("coerced")?;
    let subscription_literal_id = namespace.subscription_id("literal")?;
    contexts
        .serve(
            "contexts/states.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://example.org/{namespace}/Device",
        "status": {{
          "@id": "https://example.org/{namespace}/status",
          "@type": "@vocab"
        }},
        "active": "https://example.org/{namespace}/state/active",
        "note": "https://example.org/{namespace}/note"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device_values = Entity::builder()
        .id(device_values_id.clone())
        .entity_type(device_type.clone())
        .property("status", format!("https://example.org/{namespace}/state/active"))
        .build()?;
    body_context.entities().create(&device_values).send().await?.done()?;
    let subscription_coerced = Subscription::builder()
        .id(subscription_coerced_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&device_type).build()?).with_ids(device_values_id.clone())])
        .q(q::attr("status").eq("active").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("coerced")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription expandValues or jsonKeys), so `expandValues` is written as text.
    let expand_values: RawJson = r#""status""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_coerced.render())?.set_member("expandValues", &expand_values);
    let request = body_context
        .unchecked(&CreateSubscription {
            subscription: &subscription_coerced,
        })?
        .document(&document);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_literal = Subscription::builder()
        .id(subscription_literal_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&device_type).build()?).with_ids(device_values_id.clone())])
        .q(q::attr("status").eq("active").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("literal")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    body_context.subscriptions().create(&subscription_literal).send().await?;
    let fragment = EntityFragment::builder().property("note", "expand-values").build()?;
    body_context.entities().append_attributes(&device_values_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least("coerced", 1).await?;
    let notifications = sink.received("coerced").await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_coerced_id));
    // The literal Subscription is owed a notification of the later change to the string "active", and notifications of one subscription are delivered in order, so a notification of the first change would be in its journal before it.
    let fragment_2 = EntityFragment::builder().property("status", "active").build()?;
    body_context.entities().update_attributes(&device_values_id, &fragment_2).send().await?.done()?;
    sink.wait_for_at_least("literal", 1).await?;
    let notifications_2 = sink.received("literal").await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_2.data().as_slice(),
        nth(0, has_attribute("status", any_attribute().value("active")))
    );
    broker.subscriptions().delete(&subscription_coerced_id).send().await?;
    broker.subscriptions().delete(&subscription_literal_id).send().await?;
    Ok(())
}
