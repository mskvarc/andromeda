//! Periodic Subscriptions of more tenants than stay loaded keep within the wake bound.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use chrono::{DateTime, TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;
use tokio::time::sleep;

/// The tenants, each with one periodic Subscription.
const TENANTS: usize = 10;
/// The Subscriptions' `timeInterval`, in seconds.
const INTERVAL_SECONDS: u32 = 5;
/// The broker's `tenancy.residency.min_wake_interval`, in seconds.
const MIN_WAKE_SECONDS: i64 = 5;
/// How long the notifications are collected.
const OBSERVED: Duration = Duration::from_secs(80);

/// Pins: on a broker keeping four tenants loaded, the default tenant among them, ten tenants each with a Subscription
/// with `timeInterval` 5 are notified for 80 seconds with no gap between two notifications of one Subscription over
/// 11 seconds
/// Clause: 5.8.6, 5.2.12
/// Tenant: private, ten of them (the subject is more tenants with periodic work than the broker keeps loaded)
/// Recorded: Andromeda configuration, not v1.9.1: a tenant unloaded with a notification due is loaded again no earlier
/// than `min_wake_interval` (5 s) after it was unloaded, so a notification is late by at most that while the broker
/// loads tenants as fast as their work falls due; the bound is the interval plus that delay plus a second of the
/// tenant's load and the notification's delivery
#[gtest]
#[tokio::test]
async fn a_ntf_trg_9017_periodic_subscriptions_of_more_tenants_than_stay_loaded_keep_within_the_wake_bound() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Waker)?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let mut cleanups = Vec::with_capacity(TENANTS);
    for index in 0..TENANTS {
        let tenant = broker.with_tenant(namespace.numbered_tenant(index)?);
        let vehicle_id = namespace.urn("Vehicle", index)?;
        let subscription_id = namespace.subscription_id(index)?;
        let vehicle = Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 1)
            .build()?;
        tenant.entities().create(&vehicle).send().await?.done()?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id)])
            .time_interval(INTERVAL_SECONDS)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(index)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.subscriptions().create(&subscription).send().await?;
        cleanups.push(Cleanup::of(&tenant).subscription(&subscription_id));
    }
    sleep(OBSERVED).await;
    let bound = TimeDelta::seconds(i64::from(INTERVAL_SECONDS) + MIN_WAKE_SECONDS + 1);
    for index in 0..TENANTS {
        let notified_at = sink
            .received(index)
            .await?
            .iter()
            .map(|notification| Ok(DateTime::<Utc>::from(*notification.entities(Lossless::Normalized)?.notified_at())))
            .collect::<Result<Vec<DateTime<Utc>>>>()?;
        let gaps: Vec<TimeDelta> = notified_at.windows(2).map(|pair| pair[1] - pair[0]).collect();
        // 80 seconds hold at least six gaps at the bound; fewer means the Subscription stalled.
        expect_that!(gaps.len(), ge(6), "Subscription {index}");
        expect_that!(gaps.iter().max(), some(le(&bound)), "Subscription {index}, gaps {gaps:?}");
    }
    drop(cleanups);
    Ok(())
}
