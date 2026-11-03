//! A durable notification keeps its id when its Subscription is updated between attempts.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SubscriptionFragment},
};
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: under durable delivery, a notification whose first attempt fails and whose Subscription is then updated reaches
/// the endpoint again under the same notification `id` and a different `notifiedAt`
/// Clause: 5.3.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where a queued notification is sent with the Subscription as it stands at each attempt
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9005_a_durable_notification_keeps_its_id_when_its_subscription_is_updated_between_attempts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(500))
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
    sink.wait_for(1, 1).await?;
    let updated = SubscriptionFragment::builder().watched_attributes(["name"]).build()?;
    broker_durable.subscriptions().update(&subscription_id, &updated).send().await?;
    let attempts = sink.wait_for_at_least(1, 2).await?;
    let first = attempts.first().or_fail()?.entities(Lossless::Normalized)?;
    let second = attempts.get(1).or_fail()?.entities(Lossless::Normalized)?;
    let first_id = first.id().to_string();
    let first_notified_at = first.notified_at().to_string();
    expect_that!(second.id().to_string(), eq(&first_id));
    expect_that!(second.notified_at().to_string(), not(eq(&first_notified_at)));
    broker_durable.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
