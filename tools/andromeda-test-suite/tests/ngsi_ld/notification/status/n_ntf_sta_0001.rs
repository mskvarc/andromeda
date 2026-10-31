//! A subscription never notified reports no delivery statistics.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription that has not sent any notification reports none of the delivery statistics: no `timesSent`,
/// `timesFailed`, `lastNotification`, `lastSuccess`, `lastFailure` or notification status
/// Clause: 5.2.14.2, 5.8.3
#[gtest]
#[tokio::test]
async fn n_ntf_sta_0001_a_subscription_never_notified_reports_no_delivery_statistics() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.notification().endpoint().uri(), eq(&sink.endpoint(1)?));
    expect_that!(subscription_2.notification().output().times_sent, none());
    expect_that!(subscription_2.notification().output().times_failed, none());
    expect_that!(subscription_2.notification().output().last_notification, none());
    expect_that!(subscription_2.notification().output().last_success, none());
    expect_that!(subscription_2.notification().output().last_failure, none());
    expect_that!(subscription_2.notification().output().status, none());
    Ok(())
}
