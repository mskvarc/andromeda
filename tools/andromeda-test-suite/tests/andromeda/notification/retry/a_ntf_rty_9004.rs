//! Two changes of one durable write reach one Subscription under different ids.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: under durable delivery, a batch creating two Entities, which queues two events, reaches one Subscription in two
/// notifications under different notification `id`s
/// Clause: 5.3.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where one write queues one event per affected Entity
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9004_two_changes_of_one_durable_write_carry_a_different_id_for_one_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let buildings = (1..=2)
        .map(|n| {
            Ok(Entity::builder()
                .id(namespace.urn("Building", n)?)
                .entity_type(building_type.clone())
                .property("name", "a")
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker_durable.entities().batch_create(&buildings).send().await?.done()?;
    let received = sink.wait_for_at_least(1, 2).await?;
    let ids = received
        .iter()
        .map(|notification| Ok(notification.entities(Lossless::Normalized)?.id().to_string()))
        .collect::<Result<Vec<_>>>()?;
    expect_that!(ids.len(), eq(2));
    expect_that!(ids.first(), not(eq(ids.get(1))));
    broker_durable.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
