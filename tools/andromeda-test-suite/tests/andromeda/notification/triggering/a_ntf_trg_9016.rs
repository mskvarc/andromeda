//! A Replace that leaves an entity outside q notifies only a subscription listening to the removal.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: for a Vehicle created with state "active" and speed 80, a Replace Entity setting state to "retired" and
/// dropping speed, then one setting state back to "active", the Subscription with q state=="active" and the default
/// triggers is first notified of the Vehicle with state "active", while the one with the same q and the
/// `entityUpdated` trigger is first notified of it with state "retired"
/// Clause: 5.8.6, 5.2.12, 5.6.18
/// Recorded: Andromeda reading of v1.9.1: clause 5.8.6 matches "the subscribed Entities that changed" against the
/// filters, the state the write left; the state before a write that removed attributes is matched only for a
/// Subscription that removal is owed to (`attributeDeleted`, listed or through `entityUpdated`), since a removed
/// attribute exists only there. Notifications about one entity arrive in commit order, so the first one each
/// Subscription receives tells which Replace notified it
#[gtest]
#[tokio::test]
async fn a_ntf_trg_9016_a_replace_that_leaves_an_entity_outside_q_notifies_only_a_subscription_listening_to_the_removal() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_default_id = namespace.subscription_id("default")?;
    let subscription_removal_id = namespace.subscription_id("removal")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription_default = Subscription::builder()
        .id(subscription_default_id.clone())
        .entities([vehicle_type.as_str()])
        .q(q::attr("state").eq("active").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("default")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_default).send().await?;
    let subscription_removal = Subscription::builder()
        .id(subscription_removal_id.clone())
        .entities([vehicle_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityUpdated])
        .q(q::attr("state").eq("active").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("removal")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_removal).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("state", "active")
        .property("speed", 80)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_retired = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("state", "retired")
        .build()?;
    broker.entities().replace(&vehicle_retired).send().await?.done()?;
    let vehicle_active = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("state", "active")
        .build()?;
    broker.entities().replace(&vehicle_active).send().await?.done()?;
    let received_default = sink.wait_for_at_least("default", 1).await?;
    let notification_default = received_default.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_default.subscription_id(), eq(&subscription_default_id));
    expect_that!(
        notification_default.data().as_slice(),
        nth(0, has_attribute("state", property().value("active")))
    );
    let received_removal = sink.wait_for_at_least("removal", 1).await?;
    let notification_removal = received_removal.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_removal.subscription_id(), eq(&subscription_removal_id));
    expect_that!(
        notification_removal.data().as_slice(),
        nth(0, has_attribute("state", property().value("retired")))
    );
    broker.subscriptions().delete(&subscription_default_id).send().await?;
    broker.subscriptions().delete(&subscription_removal_id).send().await?;
    Ok(())
}
