//! A durable delivery starts due at the instant its change was committed, on the database clock.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    delivery_queue::DeliveryQueue,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    tenant_management::database_of,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: under durable delivery, the delivery a created Entity queues for a matching Subscription records as the
/// instant it is due exactly the commit instant its change event records, both read from the database clock, so a
/// broker whose clock runs ahead of its database holds no first notification back
/// Clause: 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), whose queue compares every due instant with the database clock
/// Tenant: private (the test reads its tenant's delivery queue, and only a named tenant's usage names the database
/// holding it)
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9007_a_durable_delivery_starts_due_at_the_commit_instant_of_its_change() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&broker_durable, &namespace).await?;
    let queue = DeliveryQueue::of(BrokerRole::Durable, &database_of(&broker_durable, &namespace.tenant()?).await?)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let entity = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    tenant.entities().create(&entity).send().await?.done()?;
    let delivery = retried(async || queue.deliveries_of(&subscription_id).await?.into_iter().next().or_fail()).await?;
    expect_that!(delivery.next_attempt_at, eq(delivery.committed_at));
    tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
