//! One change matched by two durable Subscriptions reaches their endpoints under different ids.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    notification_sink::NotificationSink,
    notification_uuid::uuid_version,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use uuid::Version;

/// Pins: under durable delivery, one Entity creation matched by two Subscriptions reaches their two endpoints under
/// different notification `id`s, both version 5 UUIDs
/// Clause: 5.3.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where the `id` is derived from the write, the change's position and the Subscription
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9003_one_change_matched_by_two_durable_subscriptions_carries_a_different_id_for_each() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_ids = [namespace.subscription_id(1)?, namespace.subscription_id(2)?];
    for (n, id) in [(1, &subscription_ids[0]), (2, &subscription_ids[1])] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([building_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(n)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker_durable.subscriptions().create(&subscription).send().await?;
    }
    let entity = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    broker_durable.entities().create(&entity).send().await?.done()?;
    let first = sink.wait_for(1, 1).await?.entities(Lossless::Normalized)?;
    let second = sink.wait_for(2, 1).await?.entities(Lossless::Normalized)?;
    let first_id = first.id().to_string();
    expect_that!(second.id().to_string(), not(eq(&first_id)));
    expect_that!(uuid_version(first.id()), some(eq(Version::Sha1)));
    expect_that!(uuid_version(second.id()), some(eq(Version::Sha1)));
    for id in &subscription_ids {
        broker_durable.subscriptions().delete(id).send().await?;
    }
    Ok(())
}
