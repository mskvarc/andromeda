//! A notification carrying a context on each entity is relayed to the subscriber.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    context_source::ContextSource,
    entity_lookup::with_id,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_text, placed_copy_id, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Notification of the copy posted as application/ld+json with an @context on each entity of data answers
/// 200 and the subscriber is sent both entities with their speed
/// Clause: 5.8.6, 6.3.4
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6805_a_notification_carrying_a_context_on_each_entity_is_relayed_to_the_subscriber() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let first_id = namespace.urn("Vehicle", "contexts-1")?;
    let second_id = namespace.urn("Vehicle", "contexts-2")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let core = SuiteConfig::get().map_err(Clone::clone)?.core_context().as_str();
    // The entity model writes no `@context` on an entity of `data`, so the notification is written as text.
    let data = format!(
        r#"[{{"id":"{first_id}","type":"{vehicle_type}","speed":{{"type":"Property","value":40}},"@context":"{core}"}},{{"id":"{second_id}","type":"{vehicle_type}","speed":{{"type":"Property","value":50}},"@context":"{core}"}}]"#
    );
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data(), len(eq(2)));
    expect_that!(with_id(notification.data(), &first_id), some(has_attribute("speed", any_attribute().value(40))));
    expect_that!(
        with_id(notification.data(), &second_id),
        some(has_attribute("speed", any_attribute().value(50)))
    );
    Ok(())
}
