//! Of two durable deliveries that fail together, the delivery of the earlier change is the probe.

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
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// The notifications the endpoint answers with 503: the first attempt of each of the two deliveries.
const FAILURES: u64 = 2;

/// Pins: under durable delivery, of two deliveries one batch queued for a Subscription whose endpoint answers 503 to
/// both first attempts, the delivery of the first Entity of the batch is the probe and the other is set aside behind
/// it, and once the endpoint accepts, it receives the first Entity again and then the second
/// Clause: 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where a failing endpoint holds its Subscription's other deliveries behind one probe, the probe
/// being the delivery of the earliest change, by commit instant and then by position within the write
/// Tenant: private (the test reads its tenant's delivery queue, which only a named tenant's usage names the database
/// of, and both deliveries must be claimed together, which deliveries other tests queue in a shared tenant could
/// prevent)
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9016_of_two_deliveries_failing_together_the_earlier_change_is_the_probe() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&broker_durable, &namespace).await?;
    let queue = DeliveryQueue::of(BrokerRole::Durable, &database_of(&broker_durable, &namespace.tenant()?).await?)?;
    let sink = NotificationSink::start().await?;
    let notify = || Mock::given(method("POST")).and(path("/notify/1"));
    sink.mount(notify().respond_with(ResponseTemplate::new(503)).up_to_n_times(FAILURES).with_priority(1))
        .await?;
    sink.mount(notify().respond_with(ResponseTemplate::new(200)).with_priority(2)).await?;
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
    // The queue lists the deliveries in the order of their changes, so the first is the delivery of the first Entity.
    let failed = retried(async || {
        let deliveries = queue.deliveries_of(&subscription_id).await?;
        verify_that!(deliveries.iter().filter(|delivery| delivery.attempt_count == 1).count(), eq(2))?;
        Ok(deliveries)
    })
    .await?;
    expect_that!(failed.iter().map(|delivery| delivery.held_at.is_some()).collect::<Vec<_>>(), eq(&[false, true]));
    let received = sink.wait_for_at_least(1, 4).await?;
    let mut ids = Vec::new();
    for notification in &received {
        ids.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.to_string()));
    }
    let after_the_failures = [namespace.urn("Building", 1)?.to_string(), namespace.urn("Building", 2)?.to_string()];
    expect_that!(ids, len(eq(4)));
    expect_that!(ids.get(2..), some(eq(&after_the_failures[..])));
    tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
