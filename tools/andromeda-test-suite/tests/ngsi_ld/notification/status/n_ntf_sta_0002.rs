//! A subscription recreated under a deleted id starts without statistics.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: deleting a subscription takes its delivery statistics with it: a new subscription created under the same
/// id, which has notified nothing, reports no `timesSent`, `lastNotification`, `lastSuccess` or notification status
/// Clause: 5.2.14.2, 5.8.5, 5.8.1.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_sta_0002_a_subscription_recreated_under_a_deleted_id_starts_without_statistics() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .property("name", "Hall")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    retried(async || {
        let subscription_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(subscription_2.notification().output().times_sent, some(ge(1)))?;
        verify_that!(subscription_2.notification().output().last_success, some(anything()))?;
        Ok(())
    })
    .await?;
    broker.subscriptions().delete(&subscription_id).send().await?;
    let subscription_3 = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_3).send().await?;
    let subscription_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_4.notification().endpoint().uri(), eq(&sink.endpoint(2)?));
    expect_that!(subscription_4.notification().output().times_sent, none());
    expect_that!(subscription_4.notification().output().last_notification, none());
    expect_that!(subscription_4.notification().output().last_success, none());
    expect_that!(subscription_4.notification().output().status, none());
    Ok(())
}
