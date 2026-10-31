//! Show changes on an entity creation carries no previous member.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property, relationship},
    attribute_member::Member,
    broker,
    entity_matcher::{every_instance, has_attribute},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        Entity,
        LanguageMap,
        LanguageProperty,
        Lossless,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A notification triggered by `entityCreated` on a subscription with `showChanges` carries the new Entity
/// with no `previousValue`, `previousObject`, `previousLanguageMap`, `previousJson` or `previousVocab` on any of
/// its Attributes
/// Clause: 5.8.6, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0002_show_changes_on_an_entity_creation_carries_no_previous_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let city_id = namespace.urn("City", "paris")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .relationship("locatedAt", city_id.clone())
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .geo_property("location", Shape::Point(Position::try_new(-8.5, 41.2)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("locatedAt", relationship().object(city_id)));
    for (name, _) in entity.attributes.iter() {
        expect_that!(entity, every_instance(name.as_str(), any_attribute().without(Member::Previous)));
    }
    Ok(())
}
