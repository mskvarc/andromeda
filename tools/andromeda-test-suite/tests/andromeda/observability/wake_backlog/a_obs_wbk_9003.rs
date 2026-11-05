//! A broker with no place free for a woken tenant defers no wake, and loads due tenants as fast as their work drains.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, MetricLabel, MetricsReading},
    config::BrokerRole,
    namespace::Namespace,
    notification_sink::NotificationSink,
    periodic_tenants::subscribe_periodic_tenants,
};
use googletest::prelude::*;
use std::time::Duration;
use tokio::time::sleep;

/// The tenants, each with one periodic Subscription.
const TENANTS: usize = 40;
/// The Subscriptions' `timeInterval`, in seconds.
const INTERVAL_SECONDS: u32 = 1;
/// How long the wakes are left to fall behind before the window opens.
const SETTLE: Duration = Duration::from_secs(10);
/// How long the window stays open.
const OBSERVED: Duration = Duration::from_secs(30);
/// The fewest tenants the window must load: three places, each held for a five-second woken lifetime, load 18 tenants
/// in 30 seconds; a woken tenant that gives up its place once its notification is sent lets them load three times that.
const FEWEST_LOADS: f64 = 54.0;

/// The wakes of the broker that resolved as `outcome` so far.
async fn wakes(outcome: &str) -> Result<f64> {
    Ok(MetricsReading::of(BrokerRole::Overloaded)
        .await?
        .sum_labelled(BrokerMetric::TenantWakes, MetricLabel::Outcome, outcome)
        .unwrap_or_default())
}

/// Pins: on a broker keeping four tenants loaded, the default tenant among them, a woken tenant loaded for five seconds
/// and an unloaded tenant woken again a second later, forty tenants each with a Subscription with `timeInterval` 1 keep
/// every place taken; from 10 seconds on, over 30 seconds, `andromeda_tenant_wakes_total{outcome="deferred"}` does not
/// grow, wakes are still reported overdue, and `andromeda_tenant_wakes_total{outcome="loaded"}` grows by at least 54
/// Clause: 5.8.6, 5.2.12
/// Tenant: private, forty of them (the subject is the broker's whole wake schedule)
/// Recorded: Andromeda configuration, not v1.9.1: a wake pass claims only as many due tenants as it has places for,
/// so with no place free it probes none and defers none; with every place taken, a woken tenant whose work has run
/// gives up its place before its woken lifetime ends
#[gtest]
#[tokio::test]
async fn a_obs_wbk_9003_a_broker_with_no_place_free_for_a_woken_tenant_defers_no_wake_and_loads_tenants_as_their_work_drains() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::quiet_client(BrokerRole::Overloaded)?;
    let sink = NotificationSink::start().await?;
    let periodic = subscribe_periodic_tenants(&broker, &namespace, &sink, TENANTS, INTERVAL_SECONDS).await?;
    sleep(SETTLE).await;
    let deferred_before = wakes("deferred").await?;
    let loaded_before = wakes("loaded").await?;

    sleep(OBSERVED).await;

    let deferred_after = wakes("deferred").await?;
    let loaded_after = wakes("loaded").await?;
    let overdue = MetricsReading::of(BrokerRole::Overloaded).await?.sum(BrokerMetric::OverdueWakes);
    expect_that!(deferred_after - deferred_before, approx_eq(0.0));
    expect_that!(overdue, some(gt(0.0)));
    expect_that!(loaded_after - loaded_before, ge(FEWEST_LOADS));
    drop(periodic);
    Ok(())
}
