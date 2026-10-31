//! A periodic subscription q with a language carries only the matching entity.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A periodic notification of a subscription whose q is name`[en]`=="Door" carries the Entity whose en string
/// is Door and not the one whose de string is Door
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0013_a_periodic_subscription_q_with_a_language_carries_only_the_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let door_gate_id = namespace.urn("Door", "gate")?;
    let door_type = namespace.entity_type("Door")?;
    let door_door_id = namespace.urn("Door", "door")?;
    let subscription_id = namespace.subscription_id(1)?;
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
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([door_type.as_str()])
        .time_interval(2)
        .q(q::attr("name").member("en").eq("Door").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), contains(has_id(&door_door_id)));
    Ok(())
}
