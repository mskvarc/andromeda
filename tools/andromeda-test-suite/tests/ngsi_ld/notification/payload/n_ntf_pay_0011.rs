//! Each flat join follows only the dataset instances its subscription selects.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson, Relationship},
    query::Join,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Two subscriptions with join flat select different `datasetId` instances of one Relationship: each is
/// notified of the Entity creation with the City its own instance names, and not the other
/// Clause: 5.8.6, 4.5.23.3, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0011_each_flat_join_follows_only_the_dataset_instances_its_subscription_selects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let city_2_id = namespace.urn("City", 2)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let building_id = namespace.urn("Building", 1)?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type.clone()).build()?;
    broker.entities().create(&city).send().await?.done()?;
    let city_2 = Entity::builder().id(city_2_id.clone()).entity_type(city_type.clone()).build()?;
    broker.entities().create(&city_2).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .join_level(1)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription datasetId), so `datasetId` is written as text.
    let dataset_id: RawJson = r#"["@none"]"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("datasetId", &dataset_id);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .join_level(1)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription datasetId), so `datasetId` is written as text.
    let dataset_id_2: RawJson = format!(r#"["urn:ngsi-ld:Dataset:{namespace}-alternative"]"#).parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_2.render())?.set_member("datasetId", &dataset_id_2);
    let request_2 = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .relationship("locatedAt", city_id.clone())
        .relationship(
            "locatedAt",
            Relationship::builder(city_2_id.clone()).dataset_id(namespace.dataset_id("Dataset", "alternative")?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(2)));
    expect_that!(notification.data().as_slice(), contains(has_id(&building_id)));
    expect_that!(notification.data().as_slice(), contains(has_id(&city_id)));
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.subscription_id(), eq(&subscription_2_id));
    expect_that!(notification_2.data().as_slice(), len(eq(2)));
    expect_that!(notification_2.data().as_slice(), contains(has_id(&building_id)));
    expect_that!(notification_2.data().as_slice(), contains(has_id(&city_2_id)));
    Ok(())
}
