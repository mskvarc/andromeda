//! Each notification carries the modified at of the write it notifies.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker::{self, BrokerClient},
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{DateTime, Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType, Output, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SystemAttributeReporting},
};
use std::time::Duration;
use wiremock::ResponseTemplate;

/// Pins: Three updates of one Entity, the last two committed while the endpoint still holds the notification of the
/// first, produce three notifications under `sysAttrs`, each carrying the `modifiedAt` and the `speed` its own update
/// left the Entity with
/// Clause: 5.8.6, 4.8, Table 5.2.14-1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9042_each_notification_carries_the_modified_at_of_the_write_it_notifies() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 0)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // Every answer comes late, so the notification of an update is still outstanding when the next update commits.
    sink.answer(1, ResponseTemplate::new(200).set_delay(Duration::from_millis(1500))).await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .system_attributes(SystemAttributeReporting::Include)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let mut written = Vec::with_capacity(3);
    for speed in 1..=3 {
        let fragment = EntityFragment::builder().property("speed", speed).build()?;
        broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
        written.push((speed, stored_modified_at(&broker, &vehicle_id).await?));
    }
    sink.wait_for_at_least(1, 3).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(3)));
    let mut notified = Vec::with_capacity(notifications.len());
    for received in &notifications {
        notified.push(received.entities(Lossless::Normalized)?);
    }
    let items: Vec<&Entity<Output>> = notified.iter().filter_map(|notification| notification.data().first()).collect();
    expect_that!(items, len(eq(3)));
    // Clause 5.8.6 fixes no delivery order, so each notification is paired with its update by the speed it carries.
    for (speed, modified_at) in &written {
        let item = items
            .iter()
            .find(|item| has_attribute("speed", property().value(*speed)).matches(**item).is_match())
            .or_fail()?;
        expect_that!(system_time::of_entity(item, SystemMember::ModifiedAt)?, eq(modified_at));
        expect_that!(system_time::of_attribute(item, &["speed"], SystemMember::ModifiedAt)?, eq(modified_at));
    }
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

/// The `modifiedAt` the Entity `id` reads back with under `sysAttrs`.
async fn stored_modified_at(broker: &BrokerClient, id: &EntityId) -> Result<DateTime> {
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    Ok(*system_time::of_entity(retrieved.body(), SystemMember::ModifiedAt)?)
}
