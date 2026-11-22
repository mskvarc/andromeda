//! Of the deliveries a hold releases into its Subscription's throttling window, the newest is sent once the window has
//! passed and the ones before it are dropped.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    delivery_queue::DeliveryQueue,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
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

/// The Subscription's `throttling`, in seconds.
const THROTTLING_SECONDS: u32 = 2;

/// Longer than the throttling window and the one-second longest retry step of broker-single-tenant together, so a
/// dropped delivery that was still owed would have arrived.
const SETTLED: Duration = Duration::from_secs(4);

/// The endpoint answering `status` to every notification, ahead of every mock of a larger `priority`.
fn answering(status: u16, priority: u8) -> Mock {
    Mock::given(method("POST"))
        .and(path("/notify/1"))
        .respond_with(ResponseTemplate::new(status))
        .with_priority(priority)
}

/// The Entities of every notification endpoint 1 of `sink` received, oldest first.
async fn notified(sink: &NotificationSink) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for notification in sink.received(1).await? {
        ids.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.to_string()));
    }
    Ok(ids)
}

/// Pins: under durable delivery, a Subscription with a `throttling` of 2 seconds whose first notification the endpoint
/// answers with 503 sets aside the two changes made while it is held; once its probe is delivered, the older of them
/// is dropped and the newer one is sent after the window, so the endpoint receives the first change until it accepts
/// it, then the third, once, and never the second
/// Clause: 5.2.12, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery on broker-single-tenant, where a delivery
/// a throttling window withholds is dropped for a newer one of its Subscription still queued and the newest waits the
/// window out
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9013_a_released_hold_sends_its_newest_delivery_once_the_throttling_window_has_passed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_single_tenant = broker::client(BrokerRole::SingleTenant)?;
    let queue = DeliveryQueue::of_default_tenant(BrokerRole::SingleTenant)?;
    let sink = NotificationSink::start().await?;
    sink.mount(answering(503, 3)).await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .throttling(THROTTLING_SECONDS)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_single_tenant.subscriptions().create(&subscription).send().await?;
    let building = |n: u32| -> Result<_> {
        Ok(Entity::builder()
            .id(namespace.urn("Building", n)?)
            .entity_type(building_type.clone())
            .property("name", "a")
            .build()?)
    };
    broker_single_tenant.entities().create(&building(1)?).send().await?.done()?;
    // The failure and the hold it takes commit together, so the later deliveries are queued behind the hold.
    retried(async || {
        let deliveries = queue.deliveries_of(&subscription_id).await?;
        verify_that!(deliveries.first().map(|probe| probe.attempt_count), some(ge(1)))
    })
    .await?;
    broker_single_tenant.entities().create(&building(2)?).send().await?.done()?;
    broker_single_tenant.entities().create(&building(3)?).send().await?.done()?;
    let queued = queue.deliveries_of(&subscription_id).await?;
    expect_that!(queued.iter().skip(1).map(|held| held.held_at.is_some()).collect::<Vec<_>>(), eq(&[true, true]));
    sink.mount(answering(200, 2)).await?;
    let third = namespace.urn("Building", 3)?.to_string();
    retried(async || verify_that!(notified(&sink).await?.last(), some(eq(&third)))).await?;
    let settled = sink.count_after(1, SETTLED).await?;
    let ids = notified(&sink).await?;
    let first = namespace.urn("Building", 1)?.to_string();
    expect_that!(ids, len(eq(settled)));
    expect_that!(ids.last(), some(eq(&third)));
    expect_that!(ids.iter().filter(|id| **id == third).count(), eq(1));
    expect_that!(ids.iter().filter(|id| **id == first).count(), ge(2));
    expect_that!(ids.iter().filter(|id| **id != first && **id != third).count(), eq(0));
    broker_single_tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
