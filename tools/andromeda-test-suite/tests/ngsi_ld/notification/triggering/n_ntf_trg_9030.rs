//! A periodic subscription with an interval below one second is notified at its own period.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use chrono::Utc;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, Lossless, NgsiLdMediaType, Number},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: in a tenant of its own, a Subscription with `timeInterval` 0.5 has received at least 5 notifications with a
/// `notifiedAt` within 3 s of its `createdAt`
/// Clause: 5.2.12, 5.8.6
/// Recorded: Table 5.2.12-1 allows any `timeInterval` greater than 0, and clause 5.8.6 notifies each time the interval
/// is reached; the grid points at 0.5 s to 3 s after creation are six, so five leave one sweep pass of slack
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9030_a_periodic_subscription_below_one_second_is_notified_at_its_own_period() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", "half-second")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id("half-second")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let selector = EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([selector])
        .time_interval("0.5".parse::<Number>()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("half-second")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let created_at = chrono::DateTime::<Utc>::from(stored.output().created_at.or_fail()?);
    let received = sink.wait_for_at_least("half-second", 5).await?;
    let mut notified_at = received
        .iter()
        .map(|notification| Ok(chrono::DateTime::<Utc>::from(*notification.entities(Lossless::Normalized)?.notified_at())))
        .collect::<Result<Vec<_>>>()?;
    notified_at.sort_unstable();
    let notified_at = notified_at;
    let fifth = notified_at.get(4).or_fail()?;
    expect_that!((*fifth - created_at).num_milliseconds(), le(3_000));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
