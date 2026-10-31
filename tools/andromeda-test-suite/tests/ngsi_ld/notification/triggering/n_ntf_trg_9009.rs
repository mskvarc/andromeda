//! A replace that creates updates and deletes notifies once.

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

/// Pins: A Replace Entity that updates a, creates d and removes b and c notifies a Subscription listening to
/// `attributeCreated`, `attributeUpdated` and `attributeDeleted` once, carrying a and d with their new values and b
/// and c as the NGSI-LD Null
/// Clause: 5.6.18.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9009_a_replace_that_creates_updates_and_deletes_notifies_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let np_replace_id = namespace.urn("NpReplace", 1)?;
    let np_replace_type = namespace.entity_type("NpReplace")?;
    let subscription_np_replace_id = namespace.subscription_id("np-replace")?;
    let np_replace = Entity::builder()
        .id(np_replace_id.clone())
        .entity_type(np_replace_type.clone())
        .property("a", 1)
        .property("b", 2)
        .property("c", 3)
        .build()?;
    broker.entities().create(&np_replace).send().await?.done()?;
    let subscription_np_replace = Subscription::builder()
        .id(subscription_np_replace_id.clone())
        .entities([np_replace_type.as_str()])
        .notification_triggers([
            NotificationTrigger::AttributeCreated,
            NotificationTrigger::AttributeUpdated,
            NotificationTrigger::AttributeDeleted,
        ])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-replace")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_replace).send().await?;
    let np_replace_replacement = Entity::builder()
        .id(np_replace_id.clone())
        .entity_type(np_replace_type.clone())
        .property("a", 10)
        .property("d", 4)
        .build()?;
    broker.entities().replace(&np_replace_replacement).send().await?.done()?;
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let fragment = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_replace_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-replace", &selected, 1).await?;
    let notifications = sink.received("np-replace").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("a", any_attribute().value("barrier"))));
    let notifications_2 = sink.received("np-replace").await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(
        notification_2.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&np_replace_id),
                has_attribute("a", any_attribute().value(10)),
                has_attribute("d", any_attribute().value(4)),
                has_attribute("b", any_attribute().value("urn:ngsi-ld:null")),
                has_attribute("c", any_attribute().value("urn:ngsi-ld:null"))
            )
        )
    );
    broker.subscriptions().delete(&subscription_np_replace_id).send().await?;
    Ok(())
}
