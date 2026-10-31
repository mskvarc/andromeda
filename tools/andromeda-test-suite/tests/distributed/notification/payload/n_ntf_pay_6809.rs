//! A relayed language property is notified in the language the subscription names.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    context_source::ContextSource,
    core_context::core_context,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_body, placed_copy_id, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A relayed entity whose name is a `LanguageProperty` is notified under a subscription with lang en as a
/// Property with value Tower and lang en
/// Clause: 5.8.6, 4.15, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6809_a_relayed_language_property_is_notified_in_the_language_the_subscription_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .lang("en".parse::<LanguageFilter>()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let relayed = Entity::output_builder()
        .id(namespace.urn("Vehicle", "lang")?)
        .entity_type(vehicle_type)
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("de", "Turm")])?),
        )
        .build()?;
    let body = notification_body(&namespace, &copy_id, vec![relayed], core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    // The reduced `LanguageProperty` carries a `lang` member no attribute model has, so the notification is read as received.
    expect_that!(&received.body, json_equals("$.data[0].name.type", "Property"));
    expect_that!(&received.body, json_equals("$.data[0].name.value", "Tower"));
    expect_that!(&received.body, json_equals("$.data[0].name.lang", "en"));
    Ok(())
}
