//! A past expires at is bad request data and changes nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: Update Subscription on broker A with an `expiresAt` in the past answers 400 `BadRequestData`, and the
/// subscription reads back still active
/// Clause: 5.8.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_upd_7603_a_past_expires_at_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().expires_at("2000-01-01T00:00:00Z".parse()?).build()?;
    let answer = broker.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Active)));
    expect_that!(subscription_2.expires_at(), none());
    Ok(())
}
