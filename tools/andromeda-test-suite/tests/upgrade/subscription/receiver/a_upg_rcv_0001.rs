//! A copy and the companion a hub placed on the release keep the Subscription notified once the hub is upgraded.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker::{self, BrokerClient},
    broker_process::BrokerProcess,
    config::BrokerRole,
    context_source::ContextSource,
    core_context::core_context,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_body_numbered, placed_copy, register_copy_source, register_numbered_copy_source, relay},
    source_request::SourceRequest,
    wire::{andromeda_uri, api_uri},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType},
    query::EntitySplitting,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: a Subscription the hub created on v1.0.0, whose copy it placed at a context source naming the receiver v1.0.0
/// served (`/ngsi-ld/v1/notifications`), reaches its subscriber once the hub runs the working tree over the same
/// database: the hub updates the copy at the source to name the receiver it serves (`/ngsi-ld/andromeda/v1/notifications`)
/// and relays what the source sends under it, and its companion has a source registered after the upgrade take a copy
/// naming that receiver, whose notification is relayed too
/// Clause: 5.8.1.4, 5.8.6
/// Recorded: the receiver is a route of Andromeda's own, so the hub updates every copy and companion a release placed
/// with another receiver; the update falls due when the upgraded hub first opens the tenant and is sent by the
/// background pass within a minute
#[gtest]
#[tokio::test]
async fn a_upg_rcv_0001_a_subscription_placed_at_a_source_before_the_upgrade_still_reaches_its_subscriber() -> Result<()> {
    let namespace = Namespace::fresh();
    let hub = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    register_copy_source(&hub, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .split_entities(EntitySplitting::Whole)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    hub.subscriptions().create(&subscription).send().await?;
    let placed = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    let copy_id = placed.id().cloned().or_fail()?;
    expect_that!(placed.notification().endpoint().uri().as_str(), eq(api_uri(&hub, "notifications")?.to_string()));

    BrokerProcess::of(BrokerRole::Front)?.upgrade().await?;
    hub.subscriptions().retrieve(&subscription_id).send().await?;
    let receiver = andromeda_uri(&hub, "notifications")?.to_string();
    let updated = source
        .wait_for_matching(&SourceRequest::matching(Method::PATCH, "/s1/ngsi-ld/v1/subscriptions/.+"), 1)
        .await?;
    let update = forwarded_body::subscription_fragment(updated.first().or_fail()?)?;
    expect_that!(
        update.notification().map(|notification| notification.endpoint().uri().as_str().to_owned()),
        some(eq(&receiver))
    );
    relay_vehicle(&hub, &sink, &namespace, &copy_id, 1).await?;

    register_numbered_copy_source(&hub, &source, &namespace, "s2", 2).await?;
    let placed_after = forwarded_body::subscription(&placed_copy(&source, "s2", &vehicle_type).await?)?;
    expect_that!(placed_after.notification().endpoint().uri().as_str(), eq(&receiver));
    relay_vehicle(&hub, &sink, &namespace, &placed_after.id().cloned().or_fail()?, 2).await
}

/// Relays a notification of the Vehicle numbered `number` under the copy `copy_id`, as its source sends it, and waits
/// for the subscriber to be sent it.
async fn relay_vehicle(hub: &BrokerClient, sink: &NotificationSink, namespace: &Namespace, copy_id: &SubscriptionId, number: u32) -> Result<()> {
    let vehicle_id = namespace.urn("Vehicle", number)?;
    let vehicle = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", 40)
        .build()?;
    let body = notification_body_numbered(namespace, number, copy_id, vec![vehicle], core_context()?)?;
    let answer = relay(hub, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let relayed = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects(format!("$.data[?(@.id == '{vehicle_id}')]").parse()?)], 1)
        .await?;
    verify_that!(relayed, len(eq(1)))
}
