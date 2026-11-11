//! A held Subscription's next probe is due one retry step after the database records its probe exhausted.

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
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// The first retry step of broker-durable, its default `notifications.delivery.queue.retry_interval`.
const FIRST_RETRY_STEP: TimeDelta = TimeDelta::seconds(5);

/// The endpoint answering `status` to every notification, ahead of every mock of a larger `priority`.
fn answering(status: u16, priority: u8) -> Mock {
    Mock::given(method("POST"))
        .and(path("/notify/1"))
        .respond_with(ResponseTemplate::new(status))
        .with_priority(priority)
}

/// Pins: under durable delivery, once the probe of a Subscription held behind a 503 is answered 400 and exhausted, the
/// delivery set aside behind it is due exactly 5 seconds (the first retry step) after the instant the probe was
/// exhausted, both stamped by the transaction recording the exhaustion from the database clock, and is then delivered
/// Clause: 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable) with its default 5-second first retry, where an exhausted probe hands its Subscription's hold to the
/// delivery set aside first, due one retry step later
/// Tenant: private (the test reads its tenant's delivery queue, and only a named tenant's usage names the database
/// holding it)
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9009_a_held_subscriptions_next_probe_is_due_one_retry_step_after_its_probe_is_exhausted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&broker_durable, &namespace).await?;
    let queue = DeliveryQueue::of(BrokerRole::Durable, &database_of(&broker_durable, &namespace.tenant()?).await?)?;
    let sink = NotificationSink::start().await?;
    sink.mount(answering(503, 3)).await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let building = |n: u32| -> Result<_> {
        Ok(Entity::builder()
            .id(namespace.urn("Building", n)?)
            .entity_type(building_type.clone())
            .property("name", "a")
            .build()?)
    };
    tenant.entities().create(&building(1)?).send().await?.done()?;
    // The failure and the hold it takes commit together, so the next delivery is queued behind the hold.
    retried(async || {
        let deliveries = queue.deliveries_of(&subscription_id).await?;
        verify_that!(deliveries.first().map(|probe| probe.attempt_count), some(ge(1)))
    })
    .await?;
    tenant.entities().create(&building(2)?).send().await?.done()?;
    let queued = queue.deliveries_of(&subscription_id).await?;
    expect_that!(queued.get(1).and_then(|successor| successor.held_at), some(anything()));
    sink.mount(answering(200, 2)).await?;
    sink.mount(answering(400, 1).up_to_n_times(1)).await?;
    let (exhausted_at, next_attempt_at) = retried(async || {
        let deliveries = queue.deliveries_of(&subscription_id).await?;
        let exhausted_at = deliveries.first().and_then(|probe| probe.exhausted_at).or_fail()?;
        let successor = deliveries.get(1).filter(|successor| successor.delivered_at.is_some()).or_fail()?;
        Ok((exhausted_at, successor.next_attempt_at))
    })
    .await?;
    expect_that!(next_attempt_at - exhausted_at, eq(FIRST_RETRY_STEP));
    tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
