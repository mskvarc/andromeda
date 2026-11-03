//! A durable notification the endpoint accepted but did not answer in time reaches it again under the same id.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// How long the endpoint takes to answer its first request: past the 10-second `request_timeout`, so the broker counts
/// the attempt as failed although the endpoint recorded the notification.
const LATE_ANSWER: Duration = Duration::from_secs(12);

/// Pins: under durable delivery, a notification the endpoint receives and answers 12 seconds later, after the broker
/// gave up on the attempt, reaches the endpoint again with the notification `id` of the first attempt
/// Clause: 5.3.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where a repeat after an unrecorded acknowledgement is the same notification
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9002_a_durable_notification_repeated_after_a_late_answer_keeps_its_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(200).set_delay(LATE_ANSWER))
            .up_to_n_times(1)
            .with_priority(1),
    )
    .await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let entity = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    broker_durable.entities().create(&entity).send().await?.done()?;
    let attempts = sink.wait_for_at_least(1, 2).await?;
    let first = attempts.first().or_fail()?.entities(Lossless::Normalized)?;
    let repeat = attempts.get(1).or_fail()?.entities(Lossless::Normalized)?;
    let first_id = first.id().to_string();
    expect_that!(repeat.id().to_string(), eq(&first_id));
    broker_durable.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
