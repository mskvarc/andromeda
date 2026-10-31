//! Refuses a time interval beside stored watched attributes and changes nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: a fragment adding `timeInterval` to a subscription that holds `watchedAttributes` is refused with
/// `BadRequestData` and leaves the subscription unchanged
/// Clause: 5.2.12, 5.8.2.4, 5.5.8
/// Recorded: clause 5.8.2.4 checks the restrictions of clause 5.2.12 against the Subscription Fragment, which is
/// valid on its own, and does not say what happens when merging it would leave `timeInterval` beside
/// `watchedAttributes`, which Table 5.2.12-1 forbids; Andromeda validates the merged subscription and refuses the
/// update
#[gtest]
#[tokio::test]
async fn o_sub_upd_0003_refuses_a_time_interval_beside_stored_watched_attributes_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["name", "subCategory"])
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
    expect_that!(subscription_2.watched_attributes(), len(eq(2)));
    expect_that!(subscription_2.time_interval(), none());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
