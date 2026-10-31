//! A named tenant notification carries the tenant header.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    header_condition::HeaderCondition,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::headers::NgsiLdTenant,
    model::{Entity, Lossless, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription created in a named Tenant is answered with the NGSILD-Tenant header, and every notification
/// it sends carries the header naming that Tenant
/// Clause: 6.3.14, 5.5.10, 5.8.6
/// Tenant: private (the subject is the NGSILD-Tenant header of a named Tenant)
#[gtest]
#[tokio::test]
async fn n_ten_hdr_0003_a_named_tenant_notification_carries_the_tenant_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
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
    let received = tenant.with_responses().subscriptions().create(&subscription).send().await?;
    expect_that!(
        received.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&namespace.tenant()?))
    );
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "trigger")
        .build()?;
    tenant.entities().create(&vehicle).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let fields = [HeaderCondition::Is(HeaderName::from_static("ngsild-tenant"), format!("{namespace}"))];
    let notifications = sink.received_matching(1, &[], &fields).await?;
    expect_that!(notifications, len(ge(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_id)));
    let fields_2 = [HeaderCondition::Absent(HeaderName::from_static("ngsild-tenant"))];
    let notifications_2 = sink.received_matching(1, &[], &fields_2).await?;
    expect_that!(notifications_2, len(eq(0)));
    Ok(())
}
