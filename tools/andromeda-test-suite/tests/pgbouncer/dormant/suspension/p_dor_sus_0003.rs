//! A suspended tenant is inert until it is resumed.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    tenant_management::{Activity, set_activity},
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;
use tokio::time::sleep;

/// How long after the suspension a notification the runtime had already sent may still land.
const IN_FLIGHT_GRACE: Duration = Duration::from_secs(1);

/// How long the suspended tenant is watched: more than three of its intervals.
const SUSPENDED_WINDOW: Duration = Duration::from_secs(18);

/// Pins: a Subscription with `timeInterval` 5 that has notified once sends no notification in the 18 s after its
/// tenant was Suspended (204), and notifies again once the tenant is resumed (204) and read
/// Clause: 5.8.6, 5.5.10
/// Tenant: private (the subject is a whole tenant's runtime, released by the suspension)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10 leaves
/// implementation-specific; a suspended tenant runs no background work
#[gtest]
#[tokio::test]
async fn p_dor_sus_0003_a_suspended_tenant_is_inert_until_it_is_resumed() -> Result<()> {
    let namespace = Namespace::fresh();
    let dormant = broker::client(BrokerRole::Dormant)?;
    let sink = NotificationSink::start().await?;
    let set = TenantSet::provision(&dormant, &namespace, 1, 1).await?;
    let member = set.member(0);
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("periodic")?)
        .entities([namespace.entity_type("Device")?.as_str()])
        .time_interval(5)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    member.client.subscriptions().create(&subscription).send().await?;
    sink.wait_for("periodic", 1).await?;
    let suspended = set_activity(&dormant, &member.tenant, Activity::Suspended).await?;
    expect_that!(&suspended, answered(StatusCode::NO_CONTENT));
    sleep(IN_FLIGHT_GRACE).await;
    let before = sink.received("periodic").await?.len();
    expect_that!(sink.count_after("periodic", SUSPENDED_WINDOW).await?, eq(before));
    let resumed = set_activity(&dormant, &member.tenant, Activity::Active).await?;
    expect_that!(&resumed, answered(StatusCode::NO_CONTENT));
    expect_that!(set.read(0).await.is_success(), eq(true));
    sink.wait_for("periodic", before + 1).await?;
    Ok(())
}
