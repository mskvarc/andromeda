//! Refuses a time interval beside a stored throttling and changes nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: a fragment adding `timeInterval` to a subscription that holds throttling is refused with `BadRequestData`
/// and leaves the subscription unchanged, while the same fragment on a subscription without throttling is applied
/// Clause: 5.2.12, 5.8.2.4, 5.5.8
/// Recorded: clause 5.8.2.4 checks the restrictions of clause 5.2.12 against the Subscription Fragment, which is
/// valid on its own, and does not say what happens when merging it would leave `timeInterval` beside throttling,
/// which Table 5.2.12-1 forbids; Andromeda validates the merged subscription and refuses the update
#[gtest]
#[tokio::test]
async fn o_sub_upd_0002_refuses_a_time_interval_beside_a_stored_throttling_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .throttling(30)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().time_interval(5).build()?;
    let answer = broker.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.throttling().and_then(|number| number.as_ref().as_i64()), some(eq(30)));
    expect_that!(subscription_2.time_interval(), none());
    let subscription_2_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2_2).send().await?;
    let fragment_2 = SubscriptionFragment::builder().time_interval(5).build()?;
    broker.subscriptions().update(&subscription_2_id, &fragment_2).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(subscription_3.time_interval().and_then(|number| number.as_ref().as_i64()), some(eq(5)));
    Ok(())
}
