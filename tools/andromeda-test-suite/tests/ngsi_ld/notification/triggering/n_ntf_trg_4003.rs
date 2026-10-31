//! A subscription q unequal with a language notifies an entity without that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose q is name`[en]`!="Door" notifies an entity whose `LanguageProperty` holds no en entry
/// and not an entity whose en string is Door
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_4003_a_subscription_q_unequal_with_a_language_notifies_an_entity_without_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_porte_id = namespace.urn("Building", "porte")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .q(q::attr("name").member("en").ne("Door").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building_door = Entity::builder()
        .id(building_door_id)
        .entity_type(building_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Zimmer")])?),
        )
        .build()?;
    broker.entities().create(&building_door).send().await?.done()?;
    let building_porte = Entity::builder()
        .id(building_porte_id.clone())
        .entity_type(building_type.clone())
        .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Porte")])?))
        .build()?;
    broker.entities().create(&building_porte).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    // The oldest notification is the fr entity: the entity created first holds the en string Door and sent none.
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_porte_id)));
    Ok(())
}
