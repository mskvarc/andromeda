//! Five durable subscriptions on one change each receive the changed entity.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: five Subscriptions on one Entity type, each with its own endpoint, under durable delivery: one creation
/// notifies each endpoint exactly once, and every notification carries the created Entity with its name
/// Clause: 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where one change is one queued event with a delivery per Subscription
#[gtest]
#[tokio::test]
async fn a_ntf_rty_0001_five_durable_subscriptions_on_one_change_each_receive_the_changed_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_4_id = namespace.subscription_id(4)?;
    let subscription_5_id = namespace.subscription_id(5)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription_3).send().await?;
    let subscription_4 = Subscription::builder()
        .id(subscription_4_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(4)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription_4).send().await?;
    let subscription_5 = Subscription::builder()
        .id(subscription_5_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(5)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription_5).send().await?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Building:{namespace}").parse::<EntityId>()?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    broker_durable.entities().create(&entity).send().await?.done()?;
    let selected = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Building:{namespace}' && @.name.value == 'a')]").parse()?,
    )];
    let notifications = sink.wait_for_at_least_where(1, &selected, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(1)));
    let selected_2 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Building:{namespace}' && @.name.value == 'a')]").parse()?,
    )];
    let notifications_3 = sink.wait_for_at_least_where(2, &selected_2, 1).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notifications_4 = sink.received(2).await?;
    expect_that!(notifications_4, len(eq(1)));
    let selected_3 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Building:{namespace}' && @.name.value == 'a')]").parse()?,
    )];
    let notifications_5 = sink.wait_for_at_least_where(3, &selected_3, 1).await?;
    expect_that!(notifications_5, len(eq(1)));
    let notifications_6 = sink.received(3).await?;
    expect_that!(notifications_6, len(eq(1)));
    let selected_4 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Building:{namespace}' && @.name.value == 'a')]").parse()?,
    )];
    let notifications_7 = sink.wait_for_at_least_where(4, &selected_4, 1).await?;
    expect_that!(notifications_7, len(eq(1)));
    let notifications_8 = sink.received(4).await?;
    expect_that!(notifications_8, len(eq(1)));
    let selected_5 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Building:{namespace}' && @.name.value == 'a')]").parse()?,
    )];
    let notifications_9 = sink.wait_for_at_least_where(5, &selected_5, 1).await?;
    expect_that!(notifications_9, len(eq(1)));
    let notifications_10 = sink.received(5).await?;
    expect_that!(notifications_10, len(eq(1)));
    Ok(())
}
