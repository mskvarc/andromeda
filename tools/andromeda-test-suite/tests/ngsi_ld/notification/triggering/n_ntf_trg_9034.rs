//! A slow endpoint delays no other periodic subscription.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;
use wiremock::ResponseTemplate;

/// How long the slow endpoint takes to answer each notification.
const SLOW_ANSWER: Duration = Duration::from_secs(8);

/// Pins: Subscription A with `timeInterval` 1, whose endpoint answers 200 after 8 s, and Subscription B with
/// `timeInterval` 1, created right after A on the same Entity with an endpoint answering at once: B receives at least 3
/// notifications with a `notifiedAt` within 5 s of its creation, each before A's first notification is answered
/// Clause: 5.8.6
/// Recorded: B's grid points 1 s to 5 s after its creation are five, so three leave two of slack; A's first
/// notification is answered no sooner than 8 s after its `notifiedAt`
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9034_a_slow_endpoint_delays_no_other_periodic_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    sink.answer("slow", ResponseTemplate::new(200).set_delay(SLOW_ANSWER)).await?;
    let vehicle_id = namespace.urn("Vehicle", "periodic")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let slow_id = namespace.subscription_id("slow")?;
    let prompt_id = namespace.subscription_id("prompt")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let selector = EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id);
    let slow = Subscription::builder()
        .id(slow_id.clone())
        .entities([selector.clone()])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("slow")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&slow).send().await?;
    let prompt = Subscription::builder()
        .id(prompt_id.clone())
        .entities([selector])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("prompt")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let prompt_requested = Utc::now();
    broker.subscriptions().create(&prompt).send().await?;
    let slow_first = sink.wait_for_at_least("slow", 1).await?;
    let slow_notified = chrono::DateTime::<Utc>::from(*slow_first.first().or_fail()?.entities(Lossless::Normalized)?.notified_at());
    let slow_answered = slow_notified + TimeDelta::from_std(SLOW_ANSWER)?;
    let received = sink.wait_for_at_least("prompt", 3).await?;
    let mut notified_at = received
        .iter()
        .map(|notification| Ok(chrono::DateTime::<Utc>::from(*notification.entities(Lossless::Normalized)?.notified_at())))
        .collect::<Result<Vec<_>>>()?;
    notified_at.sort_unstable();
    let notified_at = notified_at;
    let third = *notified_at.get(2).or_fail()?;
    expect_that!((third - prompt_requested).num_milliseconds(), le(5_000));
    expect_that!(third, lt(slow_answered));
    broker.subscriptions().delete(&prompt_id).send().await?;
    broker.subscriptions().delete(&slow_id).send().await?;
    Ok(())
}
