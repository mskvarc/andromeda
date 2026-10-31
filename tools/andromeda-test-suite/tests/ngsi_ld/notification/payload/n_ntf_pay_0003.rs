//! Show changes on appended attributes carries no previous member.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, property, relationship},
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{AttributeKind, Entity, EntityFragment, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A notification triggered by `attributeCreated` on a subscription with `showChanges`, after Append Entity
/// Attributes adds a Property, a Relationship and a `LanguageProperty` the Entity did not hold, carries them with
/// no previous member
/// Clause: 5.8.6, 5.2.14, 5.6.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0003_show_changes_on_appended_attributes_carries_no_previous_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let city_id = namespace.urn("City", "nantes")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::AttributeCreated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder()
        .property("name", "Tour Eiffel")
        .relationship("locatedAt", city_id.clone())
        .language_property("street", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place")])?))
        .build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, has_attribute("name", property().value("Tour Eiffel").without(Member::Previous)));
    expect_that!(entity, has_attribute("locatedAt", relationship().object(city_id).without(Member::Previous)));
    expect_that!(
        entity,
        has_attribute(
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("fr", "Grand Place")
                .without(Member::Previous)
        )
    );
    Ok(())
}
