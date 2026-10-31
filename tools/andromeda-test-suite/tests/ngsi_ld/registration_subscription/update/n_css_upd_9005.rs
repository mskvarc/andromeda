//! A past expires at is accepted and reads back expired.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: Update Context Source Registration Subscription with an `expiresAt` in the past answers 204 and the
/// subscription reads back expired with that `expiresAt`, while Update Subscription with the same fragment answers
/// 400 `BadRequestData` and leaves its `expiresAt` unchanged
/// Clause: 5.11.3.4, 5.8.1.4, 5.8.2.4, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_css_upd_9005_a_past_expires_at_is_accepted_and_reads_back_expired() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let registration_subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(2)?;
    let past = "2020-01-01T00:00:00Z".parse::<DateTime>()?;
    let future = "2099-01-01T00:00:00Z".parse::<DateTime>()?;
    let registration_subscription = Subscription::registration_builder()
        .id(registration_subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .expires_at(future)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&registration_subscription).send().await?;
    let registration_fragment = SubscriptionFragment::registration_builder().expires_at(past).build()?;
    broker
        .registration_subscriptions()
        .update(&registration_subscription_id, &registration_fragment)
        .send()
        .await?;
    let registration_subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&registration_subscription_id)
        .send()
        .await?;
    expect_that!(registration_subscription_2.expires_at(), some(eq(&past)));
    expect_that!(registration_subscription_2.output().status, some(eq(SubscriptionStatus::Expired)));
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .expires_at(future)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().expires_at(past).build()?;
    let answer = broker.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.expires_at(), some(eq(&future)));
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Active)));
    broker.registration_subscriptions().delete(&registration_subscription_id).send().await?;
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
