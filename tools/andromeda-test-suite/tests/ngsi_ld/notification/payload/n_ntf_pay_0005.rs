//! A periodic notification carries only the selected dataset instances.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Property, Relationship},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A periodic notification of a subscription whose `datasetId` names one dataset carries, of an Entity
/// holding three instances of a Property and of a Relationship, only the instance of each with that `datasetId`
/// Clause: 5.8.6, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0005_a_periodic_notification_carries_only_the_selected_dataset_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let french = namespace.dataset_id("Dataset", "french-name")?;
    let german = namespace.dataset_id("Dataset", "german-name")?;
    let spanish = namespace.dataset_id("Dataset", "spanish-name")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("name", Property::builder("Tour Eiffel").dataset_id(french.clone()))
        .property("name", Property::builder("Eiffelturm").dataset_id(german))
        .relationship("locatedAt", namespace.urn("City", "paris")?)
        .relationship("locatedAt", Relationship::builder(namespace.urn("Ville", "paris")?).dataset_id(french.clone()))
        .relationship("locatedAt", Relationship::builder(namespace.urn("Ciudad", "paris")?).dataset_id(spanish))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .time_interval(2)
        .dataset_ids(vec![french.clone()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, has_instances("name", 1));
    expect_that!(entity, has_attribute("name", property().value("Tour Eiffel").with_dataset_id(french.clone())));
    expect_that!(entity, has_instances("locatedAt", 1));
    expect_that!(
        entity,
        has_attribute("locatedAt", relationship().object(namespace.urn("Ville", "paris")?).with_dataset_id(french))
    );
    Ok(())
}
