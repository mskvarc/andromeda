//! A failed durable delivery is due again one retry step after the database records its failure.

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
use chrono::TimeDelta;
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use wiremock::ResponseTemplate;

/// The first retry step of broker-durable, its default `notifications.delivery.queue.retry_interval`.
const FIRST_RETRY_STEP: TimeDelta = TimeDelta::seconds(5);

/// Pins: under durable delivery, of two deliveries one batch queued for a Subscription whose endpoint answers 503 to
/// both first attempts, the one set aside behind the other is due again exactly 5 seconds (the first retry step) after
/// the instant it was set aside, which the transaction recording its failure stamps from the database clock
/// Clause: 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable) with its default 5-second first retry, where a failing endpoint holds its Subscription's other
/// deliveries behind one probe and a failure is due again one retry step after the database records it
/// Tenant: private (the test reads its tenant's delivery queue, which only a named tenant's usage names the database
/// of, and both deliveries must be claimed together, which deliveries other tests queue in a shared tenant could
/// prevent)
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9008_a_failed_durable_delivery_is_due_again_one_retry_step_after_its_failure_is_recorded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&broker_durable, &namespace).await?;
    let queue = DeliveryQueue::of(BrokerRole::Durable, &database_of(&broker_durable, &namespace.tenant()?).await?)?;
    let sink = NotificationSink::start().await?;
    sink.answer(1, ResponseTemplate::new(503)).await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let buildings = (1..=2)
        .map(|n| -> Result<_> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", n)?)
                .entity_type(building_type.clone())
                .property("name", "a")
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&buildings).send().await?.done()?;
    let set_aside = retried(async || {
        queue
            .deliveries_of(&subscription_id)
            .await?
            .into_iter()
            .find(|delivery| delivery.attempt_count == 1 && delivery.held_at.is_some())
            .or_fail()
    })
    .await?;
    let held_at = set_aside.held_at.or_fail()?;
    expect_that!(set_aside.next_attempt_at - held_at, eq(FIRST_RETRY_STEP));
    tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
