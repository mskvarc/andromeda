//! A creation the broker defers to its side-effect queue (a `Prefer: respond-async` batch) and the one notification it
//! owes (clause 5.8.6): one scenario a broker answers the same in either delivery mode, so each mode runs it with a
//! broker of its own.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    notification_sink::NotificationSink,
    respond_async::{async_applied, respond_async},
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::BatchCreate,
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;

/// How long the notifications must stay at one once it arrived.
const QUIET: Duration = Duration::from_secs(8);

/// In a tenant of its own on the broker `broker` speaks to, subscribes to the creation of the tenant's Devices, creates
/// one Device with a `Prefer: respond-async` Batch Entity Create, and expects the batch answered 201 with the
/// preference applied and exactly one notification, of that Device alone, in the 8 s after it.
///
/// # Errors
///
/// Fails when a value cannot be built, a request is refused or the notification does not arrive.
pub async fn expect_deferred_creation_notified_once(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    let sink = NotificationSink::start().await?;
    let set = TenantSet::provision(broker, namespace, 1, 1).await?;
    let tenant = &set.member(0).client;
    let device_type = namespace.entity_type("Device")?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("created")?)
        .entities([device_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("created")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let deferred_id = namespace.urn("Device", "deferred")?;
    let deferred = [Entity::builder()
        .id(deferred_id.clone()) // clone: the notification is matched to the Device by its id
        .entity_type(device_type)
        .property("reading", 1)
        .build()?];
    let request = respond_async(tenant.unchecked(&BatchCreate::builder().entities(&deferred).build())?);
    let answer = tenant.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::CREATED));
    sink.wait_for("created", 1).await?;
    expect_that!(sink.count_after("created", QUIET).await?, eq(1));
    let notifications = sink.received("created").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    verify_that!(
        notification.data().iter().map(|entity| &entity.id).collect::<Vec<_>>(),
        elements_are![eq(&&deferred_id)]
    )
}
