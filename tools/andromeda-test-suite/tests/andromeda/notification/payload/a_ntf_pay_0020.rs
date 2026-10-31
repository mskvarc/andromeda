//! A durable update with show changes carries the previous value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: under durable delivery an update notified with `showChanges` carries the new value and the value before
/// the update
/// Clause: 5.2.14.1
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where the queued event holds the previous state apart from the current one
#[gtest]
#[tokio::test]
async fn a_ntf_pay_0020_a_durable_update_with_show_changes_carries_the_previous_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let ngsi_ld_building_id = format!("urn:ngsi-ld:Building:{namespace}").parse::<EntityId>()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::AttributeUpdated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Building:{namespace}").parse::<EntityId>()?)
        .entity_type(building_type.clone())
        .property("temperature", 20)
        .build()?;
    broker_durable.entities().create(&entity).send().await?.done()?;
    let fragment = EntityFragment::builder().property("temperature", 21).build()?;
    broker_durable
        .entities()
        .update_attributes(&ngsi_ld_building_id, &fragment)
        .send()
        .await?
        .done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&ngsi_ld_building_id),
                has_attribute("temperature", any_attribute().value(21).previous_value(20))
            )
        )
    );
    Ok(())
}
