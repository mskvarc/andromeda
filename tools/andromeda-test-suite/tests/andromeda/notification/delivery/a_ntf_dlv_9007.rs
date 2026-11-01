//! An endpoint that never answers leaves the other Subscriptions of its tenant their delivery slots.

use andromeda_test_suite::{broker, config::BrokerRole, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;
use tokio::time::{Instant, sleep, timeout};
use wiremock::ResponseTemplate;

/// The Stops whose creation queues a delivery to the silent endpoint: three times the 20 deliveries a drain sends at
/// once (`notifications.delivery.max_concurrent`), so a drain that sent them in turn would hold every slot for three
/// 10-second `request_timeout`s.
const BACKLOG: u32 = 60;

/// How long the silent endpoint holds a request before it would answer, far past the broker's `request_timeout`.
const SILENCE: Duration = Duration::from_secs(120);

/// How soon the answering endpoint is notified of the Bus written behind the backlog.
const NOTIFIED_WITHIN: Duration = Duration::from_secs(5);

/// How long the silent endpoint is watched once it has failed: the first retry comes 5 seconds after the failure and is
/// held for the 10-second `request_timeout`, so the window sees that one attempt and not the one after it.
const WATCHED: Duration = Duration::from_secs(12);

/// Pins: Under durable delivery, once a Subscription's endpoint that accepts the connection and never answers has
/// failed, a Bus written behind the 60 deliveries queued for it notifies another Subscription of the tenant within
/// 5 seconds, and the silent endpoint receives at most two further attempts in the 12 seconds after it failed
/// Clause: 5.8.6
/// Tenant: private (the deliveries of one tenant share its 20 delivery slots, so the silent endpoint would hold back
/// the notifications of every other test in the shared tenant for as long as it is attempted)
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable) with its default 20 slots, 10-second request timeout and 5-second first retry, where a failing
/// endpoint holds its Subscription's other deliveries behind one probe
#[gtest]
#[tokio::test]
async fn a_ntf_dlv_9007_an_endpoint_that_never_answers_leaves_the_other_subscriptions_their_delivery_slots() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&broker_durable, &namespace).await?;
    let sink = NotificationSink::start().await?;
    sink.answer("silent", ResponseTemplate::new(204).set_delay(SILENCE)).await?;
    let stop_type = namespace.entity_type("Stop")?;
    let bus_type = namespace.entity_type("Bus")?;
    let subscription_silent_id = namespace.subscription_id("silent")?;
    let subscription_answering_id = namespace.subscription_id("answering")?;
    let subscription_silent = Subscription::builder()
        .id(subscription_silent_id.clone())
        .entities([stop_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("silent")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_silent).send().await?;
    let subscription_answering = Subscription::builder()
        .id(subscription_answering_id.clone())
        .entities([bus_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("answering")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_answering).send().await?;
    let stops = (1..=BACKLOG)
        .map(|n| {
            Ok(Entity::builder()
                .id(namespace.urn("Stop", n)?)
                .entity_type(stop_type.clone())
                .property("round", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&stops).send().await?.done()?;
    retried(async || {
        let subscription = tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_silent_id)
            .send()
            .await?;
        verify_that!(subscription.notification().output().times_failed, some(ge(1)))
    })
    .await?;
    let failed_at = Instant::now();
    let attempted = sink.received("silent").await?.len();
    let bus = Entity::builder()
        .id(namespace.urn("Bus", 1)?)
        .entity_type(bus_type.clone())
        .property("line", 7)
        .build()?;
    tenant.entities().create(&bus).send().await?.done()?;
    let notified = timeout(NOTIFIED_WITHIN, sink.wait_for_at_least("answering", 1)).await;
    expect_that!(notified, ok(ok(len(eq(1)))));
    sleep(WATCHED.saturating_sub(failed_at.elapsed())).await;
    let attempted_since = sink.received("silent").await?.len().saturating_sub(attempted);
    expect_that!(attempted_since, le(2));
    tenant.subscriptions().delete(&subscription_silent_id).send().await?;
    tenant.subscriptions().delete(&subscription_answering_id).send().await?;
    Ok(())
}
