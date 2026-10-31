//! A default tenant notification carries no tenant header.

use andromeda_test_suite::{
    broker,
    header_condition::HeaderCondition,
    header_matcher::absent,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::headers::NgsiLdTenant,
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription created in the default Tenant is answered without an NGSILD-Tenant header, and the notifications
/// it sends carry none either
/// Clause: 6.3.14, 5.5.10, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ten_hdr_0002_a_default_tenant_notification_carries_no_tenant_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let received = broker.with_responses().subscriptions().create(&subscription).send().await?;
    expect_that!(received.head.typed::<NgsiLdTenant>(), absent());
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type).property("name", "trigger").build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let fields = [HeaderCondition::Absent(HeaderName::from_static("ngsild-tenant"))];
    let notifications = sink.received_matching(1, &[], &fields).await?;
    expect_that!(notifications, len(ge(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    let fields_2 = [HeaderCondition::Contains(HeaderName::from_static("ngsild-tenant"), String::new())];
    let notifications_2 = sink.received_matching(1, &[], &fields_2).await?;
    expect_that!(notifications_2, len(eq(0)));
    Ok(())
}
