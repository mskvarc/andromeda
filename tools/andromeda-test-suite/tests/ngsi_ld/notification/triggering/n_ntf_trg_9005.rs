//! Entity and attribute triggers together notify a create once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A Subscription listing `entityCreated`, `attributeCreated` and `attributeUpdated` is notified once of the
/// creation of an Entity with three attributes
/// Clause: 5.8.6, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9005_entity_and_attribute_triggers_together_notify_a_create_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_np_triggers_id = namespace.subscription_id("np-triggers")?;
    let np_triggers_type = namespace.entity_type("NpTriggers")?;
    let np_triggers_id = namespace.urn("NpTriggers", 1)?;
    let subscription_np_triggers = Subscription::builder()
        .id(subscription_np_triggers_id.clone())
        .entities([np_triggers_type.as_str()])
        .notification_triggers([
            NotificationTrigger::EntityCreated,
            NotificationTrigger::AttributeCreated,
            NotificationTrigger::AttributeUpdated,
        ])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-triggers")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_triggers).send().await?;
    let np_triggers = Entity::builder()
        .id(np_triggers_id.clone())
        .entity_type(np_triggers_type.clone())
        .property("a", 1)
        .property("b", 2)
        .property("c", 3)
        .build()?;
    broker.entities().create(&np_triggers).send().await?.done()?;
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let fragment = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_triggers_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-triggers", &selected, 1).await?;
    let notifications = sink.received("np-triggers").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("a", any_attribute().value("barrier"))));
    let notifications_2 = sink.received("np-triggers").await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&np_triggers_id)));
    broker.subscriptions().delete(&subscription_np_triggers_id).send().await?;
    Ok(())
}
