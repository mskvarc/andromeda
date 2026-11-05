//! Periodic Subscriptions of more tenants than the broker can load are reported as overdue wakes.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, MetricsReading},
    config::BrokerRole,
    namespace::Namespace,
    notification_sink::NotificationSink,
    periodic_tenants::subscribe_periodic_tenants,
};
use googletest::prelude::*;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// The tenants, each with one periodic Subscription.
const TENANTS: usize = 40;
/// The Subscriptions' `timeInterval`, in seconds.
const INTERVAL_SECONDS: u32 = 1;
/// How long the wakes are left to fall behind before the gauges are read.
const SETTLE: Duration = Duration::from_secs(20);
/// How long the gauges are read.
const OBSERVED: Duration = Duration::from_secs(40);
/// The time between two readings.
const READING_INTERVAL: Duration = Duration::from_secs(5);

/// Pins: on a broker keeping four tenants loaded, the default tenant among them, a woken tenant loaded for five seconds
/// and an unloaded tenant woken again a second later, forty tenants each with a Subscription with `timeInterval` 1
/// leave wakes overdue: from 20 seconds on, every reading of the metrics over 40 seconds has
/// `andromeda_tenant_overdue_wakes` and `andromeda_tenant_overdue_wake_max_age_seconds` above zero
/// Clause: 5.8.6, 5.2.12
/// Tenant: private, forty of them (the subject is more tenants with periodic work than the broker can load)
/// Recorded: Andromeda configuration, not v1.9.1: with every place taken, a tenant loaded for a periodic notification
/// holds one of the three places beside the default tenant until its notification is sent, so each of its wakes costs a
/// load; forty tenants notified every second need forty loads a second, far more than three places serve
#[gtest]
#[tokio::test]
async fn a_obs_wbk_9001_periodic_subscriptions_of_more_tenants_than_the_broker_can_load_are_reported_overdue() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::quiet_client(BrokerRole::Backlog)?;
    let sink = NotificationSink::start().await?;
    let periodic = subscribe_periodic_tenants(&broker, &namespace, &sink, TENANTS, INTERVAL_SECONDS).await?;
    sleep(SETTLE).await;
    let end = Instant::now() + OBSERVED;
    while Instant::now() < end {
        let reading = MetricsReading::of(BrokerRole::Backlog).await?;
        expect_that!(reading.sum(BrokerMetric::OverdueWakes), some(gt(0.0)));
        expect_that!(reading.sum(BrokerMetric::OverdueWakeMaxAgeSeconds), some(gt(0.0)));
        sleep(READING_INTERVAL).await;
    }
    drop(periodic);
    Ok(())
}
