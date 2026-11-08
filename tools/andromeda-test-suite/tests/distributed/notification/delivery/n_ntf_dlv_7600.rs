//! A notification of a tenant carries the tenant header.

use andromeda_test_suite::{broker, header_condition::HeaderCondition, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a creation on broker A in a private Tenant, for a subscription of that Tenant, is
/// delivered with an NGSILD-Tenant header naming the Tenant
/// Clause: 5.8.6, 6.3.4, 4.14
/// Tenant: private (the subject is the NGSILD-Tenant header of a notification of a Tenant)
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_7600_a_notification_of_a_tenant_carries_the_tenant_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    tenant.entities().create(&vehicle).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let fields = [HeaderCondition::Is(HeaderName::from_static("ngsild-tenant"), format!("{namespace}"))];
    let notifications = sink.received_matching(1, &[], &fields).await?;
    expect_that!(notifications, len(ge(1)));
    Ok(())
}
