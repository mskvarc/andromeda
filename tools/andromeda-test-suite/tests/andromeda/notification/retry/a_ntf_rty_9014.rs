//! A delivery an exhausted probe hands its hold to, whose change was queued before the failure that opened the
//! endpoint's cooldown, is sent once the cooldown has passed.

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

/// The endpoint's `cooldown`, in milliseconds: longer than the 100 ms first retry step the successor of an exhausted
/// probe waits on broker-single-tenant, so the successor meets the cooldown its probe's last failure opened.
const COOLDOWN_MILLISECONDS: u32 = 1_000;

/// The attempts broker-single-tenant makes of a notification its endpoint answers with 503.
const ATTEMPTS: usize = 4;

/// Longer than the cooldown and the one-second longest retry step of broker-single-tenant together, so an attempt
/// still due would have arrived.
const SETTLED: Duration = Duration::from_secs(3);

/// Pins: under durable delivery, a notification whose endpoint has a `cooldown` of 1000 ms and answers its four
/// attempts with 503 is exhausted and hands its Subscription's hold to the delivery set aside behind it; that
/// delivery's change was made before the last failure, so the cooldown that failure opened delays it rather than
/// withholding it, and the endpoint receives it after the four attempts of the first
/// Clause: 5.2.15, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery on broker-single-tenant, where a cooldown
/// withholds for good only a delivery whose change was made after the failure that opened it
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9014_the_successor_of_an_exhausted_probe_is_sent_once_the_cooldown_has_passed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_single_tenant = broker::client(BrokerRole::SingleTenant)?;
    let queue = DeliveryQueue::of_default_tenant(BrokerRole::SingleTenant)?;
    let sink = NotificationSink::start().await?;
    let notify = || Mock::given(method("POST")).and(path("/notify/1"));
    sink.mount(
        notify()
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(u64::try_from(ATTEMPTS)?)
            .with_priority(1),
    )
    .await?;
    sink.mount(notify().respond_with(ResponseTemplate::new(200)).with_priority(2)).await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .cooldown(COOLDOWN_MILLISECONDS)
                    .build()?,
            )
            .build()?,
        )
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
    // The failure and the hold it takes commit together, so the next delivery is queued behind the hold.
    retried(async || {
        let deliveries = queue.deliveries_of(&subscription_id).await?;
        verify_that!(deliveries.first().map(|probe| probe.attempt_count), some(ge(1)))
    })
    .await?;
    broker_single_tenant.entities().create(&building(2)?).send().await?.done()?;
    let queued = queue.deliveries_of(&subscription_id).await?;
    expect_that!(queued.get(1).and_then(|successor| successor.held_at), some(anything()));
    let received = sink.wait_for_at_least(1, ATTEMPTS + 1).await?;
    expect_that!(sink.count_after(1, SETTLED).await?, eq(ATTEMPTS + 1));
    let mut ids = Vec::new();
    for notification in &received {
        ids.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.to_string()));
    }
    let mut expected = vec![namespace.urn("Building", 1)?.to_string(); ATTEMPTS];
    expected.push(namespace.urn("Building", 2)?.to_string());
    expect_that!(ids, eq(&expected));
    broker_single_tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
