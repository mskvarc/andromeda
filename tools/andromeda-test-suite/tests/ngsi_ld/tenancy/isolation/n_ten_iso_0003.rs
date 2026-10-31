//! A subscription is not notified of another tenant s entities.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription in one Tenant is notified of a matching entity created in its own Tenant and never of one
/// created in another Tenant
/// Clause: 4.14, 5.5.10, 5.8.6
/// Tenant: private (the subject is two Tenants' subscriptions and entities)
#[gtest]
#[tokio::test]
async fn n_ten_iso_0003_a_subscription_is_not_notified_of_another_tenant_s_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let b_tenant = broker.with_tenant(format!("{namespace}-b").parse()?);
    let vehicle_b1_id = namespace.urn("Vehicle", "b1")?;
    let vehicle_a1_id = namespace.urn("Vehicle", "a1")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([provisioning_sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let vehicle_b1 = Entity::builder()
        .id(vehicle_b1_id)
        .entity_type(vehicle_type.clone())
        .property("name", "tenant-b")
        .build()?;
    b_tenant.entities().create(&vehicle_b1).send().await?.done()?;
    let vehicle_a1 = Entity::builder()
        .id(vehicle_a1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "tenant-a")
        .build()?;
    tenant.entities().create(&vehicle_a1).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    // The other Tenant's entity was created first, so a notification for it would be the oldest one.
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_a1_id)));
    let selected = [BodyPattern::Contains(format!("urn:ngsi-ld:Vehicle:{namespace}-b1"))];
    let notifications_2 = sink.received_where(1, &selected).await?;
    expect_that!(notifications_2, len(eq(0)));
    Ok(())
}
