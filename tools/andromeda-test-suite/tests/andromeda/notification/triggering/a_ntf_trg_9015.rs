//! A periodic subscription on a resting tenant notifies within its interval.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    clock_wait::wait_past,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: In a tenant idle for four seconds, a Subscription with `timeInterval` 2 created then sends its first
/// notification within five seconds
/// Clause: 5.8.6, 5.2.12
/// Tenant: private (the subject is a tenant with no outstanding obligation, so its background work is resting when
/// the obligation is created; the shared tenant always has some)
/// Recorded: Andromeda configuration, not v1.9.1: the stack sweeps every second
/// (`temporal.expiration_check_interval`), and a resting tenant waits up to sixteen sweeps; the bound is the
/// obligation's own time plus three sweeps
#[gtest]
#[tokio::test]
async fn a_ntf_trg_9015_a_periodic_subscription_on_a_resting_tenant_notifies_within_its_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let vehicle_periodic_id = namespace.urn("Vehicle", "periodic")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_periodic_id = namespace.subscription_id("periodic")?;
    // A registration created and deleted again brings the tenant into existence and leaves nothing in it.
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let vehicle_periodic = Entity::builder()
        .id(vehicle_periodic_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    tenant.entities().create(&vehicle_periodic).send().await?.done()?;
    let slept_until = (Utc::now() + TimeDelta::seconds(4)).timestamp();
    wait_past(slept_until).await;
    let subscription_periodic = Subscription::builder()
        .id(subscription_periodic_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_periodic_id.clone())])
        .time_interval(2)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_periodic).send().await?;
    // Twenty tries 250 ms apart bound the wait at the interval plus three sweeps.
    sink.wait_for_at_least("periodic", 1).await?;
    let notifications = sink.received("periodic").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_periodic_id));
    tenant.subscriptions().delete(&subscription_periodic_id).send().await?;
    Ok(())
}
