//! A subscription q equal with a language reads only that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A subscription whose q is name`[en]`=="Door" is not notified of an Entity whose de string is Door and is
/// notified of one whose en string is Door
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0010_a_subscription_q_equal_with_a_language_reads_only_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let door_type = namespace.entity_type("Door")?;
    let door_gate_id = namespace.urn("Door", "gate")?;
    let door_door_id = namespace.urn("Door", "door")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([door_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("name").member("en").eq("Door").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let door_gate = Entity::builder()
        .id(door_gate_id)
        .entity_type(door_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Gate"), ("de", "Door")])?),
        )
        .build()?;
    broker.entities().create(&door_gate).send().await?.done()?;
    let door_door = Entity::builder()
        .id(door_door_id.clone())
        .entity_type(door_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Tuer")])?),
        )
        .build()?;
    broker.entities().create(&door_door).send().await?.done()?;
    // One notification, of the Entity created second: the first sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&door_door_id)));
    Ok(())
}
