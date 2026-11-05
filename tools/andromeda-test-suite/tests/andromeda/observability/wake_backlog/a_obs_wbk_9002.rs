//! Periodic Subscriptions of tenants the broker keeps loaded leave no wake overdue.

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
const INTERVAL_SECONDS: u32 = 5;
/// How long the Subscriptions run before the gauges are read: one wake pass runs at least every five seconds.
const SETTLE: Duration = Duration::from_secs(10);
/// How long the gauges are read.
const OBSERVED: Duration = Duration::from_secs(30);
/// The time between two readings.
const READING_INTERVAL: Duration = Duration::from_secs(5);

/// Pins: on a broker keeping 41 tenants loaded, the default tenant among them, a woken tenant loaded for five seconds
/// and an unloaded tenant woken again five seconds later, forty tenants each with a Subscription with `timeInterval` 5
/// leave no wake overdue: from 10 seconds on, every reading of the metrics over 30 seconds has
/// `andromeda_tenant_overdue_wakes` and `andromeda_tenant_overdue_wake_max_age_seconds` at zero
/// Clause: 5.8.6, 5.2.12
/// Tenant: private, forty of them (the subject is the broker's whole wake schedule)
/// Recorded: Andromeda configuration, not v1.9.1: every tenant stays loaded, so no wake is due
#[gtest]
#[tokio::test]
async fn a_obs_wbk_9002_periodic_subscriptions_of_tenants_the_broker_keeps_loaded_leave_no_wake_overdue() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::quiet_client(BrokerRole::Roomy)?;
    let sink = NotificationSink::start().await?;
    let periodic = subscribe_periodic_tenants(&broker, &namespace, &sink, TENANTS, INTERVAL_SECONDS).await?;
    sleep(SETTLE).await;
    let end = Instant::now() + OBSERVED;
    while Instant::now() < end {
        let reading = MetricsReading::of(BrokerRole::Roomy).await?;
        expect_that!(reading.sum(BrokerMetric::OverdueWakes), some(eq(0.0)));
        expect_that!(reading.sum(BrokerMetric::OverdueWakeMaxAgeSeconds), some(eq(0.0)));
        sleep(READING_INTERVAL).await;
    }
    drop(periodic);
    Ok(())
}
