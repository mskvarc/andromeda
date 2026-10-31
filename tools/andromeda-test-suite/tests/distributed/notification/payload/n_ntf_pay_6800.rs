//! A deleted attribute is notified with the null uri as value and in the language map.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{AttributeName, Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Property},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: With `showChanges`, deleting a Property, a Property holding `observedAt` and a `LanguageProperty` notifies
/// each with urn:ngsi-ld:null as value, and for the `LanguageProperty` as the @none language map
/// Clause: 5.8.6, 4.5.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6800_a_deleted_attribute_is_notified_with_the_null_uri_as_value_and_in_the_language_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("colour", "red")
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Main Street"), ("fr", "Rue Principale")])?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["colour"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id)
        .entities([vehicle_type.as_str()])
        .watched_attributes(["street"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_3).send().await?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"colour".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"street".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, has_attribute("colour", any_attribute().value("urn:ngsi-ld:null")))
    );
    let notifications_2 = sink.wait_for_at_least(2, 1).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_2.data().as_slice(),
        nth(0, has_attribute("speed", any_attribute().value("urn:ngsi-ld:null")))
    );
    let notifications_3 = sink.wait_for_at_least(3, 1).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_3 = notifications_3.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_3.data().as_slice(),
        nth(0, has_attribute("street", any_attribute().language("@none", "urn:ngsi-ld:null")))
    );
    Ok(())
}
