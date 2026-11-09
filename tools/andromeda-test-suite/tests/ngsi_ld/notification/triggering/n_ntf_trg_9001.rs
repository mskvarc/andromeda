//! A deletion is filtered on the state it removed.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    generated_payload::ring_polygon,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Entity of a large Entity whose state passes the q of one `entityDeleted` Subscription and fails the
/// q of another notifies the first once with `deletedAt` and never the second, as later deletions of Entities in
/// the other state show
/// Clause: 5.8.6, 5.6.6, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9001_a_deletion_is_filtered_on_the_state_it_removed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_retired_id = namespace.subscription_id("retired")?;
    let probe_type = namespace.entity_type("Probe")?;
    let subscription_active_id = namespace.subscription_id("active")?;
    let probe_notified_id = namespace.urn("Probe", "notified")?;
    let probe_sentinel_active_id = namespace.urn("Probe", "sentinel-active")?;
    let probe_sentinel_retired_id = namespace.urn("Probe", "sentinel-retired")?;
    let subscription_retired = Subscription::builder()
        .id(subscription_retired_id.clone())
        .entities([probe_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityDeleted])
        .q(q::attr("state").eq("retired").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("retired")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_retired).send().await?;
    let subscription_active = Subscription::builder()
        .id(subscription_active_id.clone())
        .entities([probe_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityDeleted])
        .q(q::attr("state").eq("active").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("active")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_active).send().await?;
    let probe_notified = Entity::builder()
        .id(probe_notified_id.clone())
        .entity_type(probe_type.clone())
        .property("state", "retired")
        .geo_property("location", ring_polygon(10_000)?)
        .build()?;
    broker.entities().create(&probe_notified).send().await?.done()?;
    broker.entities().delete(&probe_notified_id).send().await?.done()?;
    let probe_sentinel_active = Entity::builder()
        .id(probe_sentinel_active_id.clone())
        .entity_type(probe_type.clone())
        .property("state", "active")
        .geo_property("location", ring_polygon(10)?)
        .build()?;
    broker.entities().create(&probe_sentinel_active).send().await?.done()?;
    broker.entities().delete(&probe_sentinel_active_id).send().await?.done()?;
    let probe_sentinel_retired = Entity::builder()
        .id(probe_sentinel_retired_id.clone())
        .entity_type(probe_type.clone())
        .property("state", "retired")
        .geo_property("location", ring_polygon(10)?)
        .build()?;
    broker.entities().create(&probe_sentinel_retired).send().await?.done()?;
    broker.entities().delete(&probe_sentinel_retired_id).send().await?.done()?;
    sink.wait_for_at_least("retired", 2).await?;
    let notifications = sink.received("retired").await?;
    expect_that!(notifications, len(eq(2)));
    let selected = [BodyPattern::Contains(format!("urn:ngsi-ld:Probe:{namespace}-notified"))];
    let notifications_2 = sink.received_where("retired", &selected).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    let item = notification.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::DeletedAt).ok(), some(anything()));
    let selected_2 = [BodyPattern::Contains(format!("urn:ngsi-ld:Probe:{namespace}-sentinel-retired"))];
    let notifications_3 = sink.received_where("retired", &selected_2).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_2 = notifications_3.first().or_fail()?.entities(Lossless::Normalized)?;
    let item_2 = notification_2.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item_2, SystemMember::DeletedAt).ok(), some(anything()));
    sink.wait_for_at_least("active", 1).await?;
    let notifications_4 = sink.received("active").await?;
    expect_that!(notifications_4, len(eq(1)));
    let selected_3 = [BodyPattern::Contains(format!("urn:ngsi-ld:Probe:{namespace}-sentinel-active"))];
    let notifications_5 = sink.received_where("active", &selected_3).await?;
    expect_that!(notifications_5, len(eq(1)));
    let notification_3 = notifications_5.first().or_fail()?.entities(Lossless::Normalized)?;
    let item_3 = notification_3.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item_3, SystemMember::DeletedAt).ok(), some(anything()));
    broker.subscriptions().delete(&subscription_retired_id).send().await?;
    broker.subscriptions().delete(&subscription_active_id).send().await?;
    Ok(())
}
