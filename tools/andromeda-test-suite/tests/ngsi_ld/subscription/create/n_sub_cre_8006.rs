//! A temporal q time at with a numeric offset is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose `temporalQ` carries a `timeAt` with a numeric offset in place of Z (+01:00, or
/// +00:00 naming UTC) answers 400 `BadRequestData`, and no Subscription exists
/// Clause: 4.6.3, 5.2.12, 5.2.21, 5.8.1.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_8006_a_temporal_q_time_at_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (member `temporalQ` is not allowed here), so `temporalQ` is written as text.
    let temporal_q: RawJson = r#"{"timerel": "after", "timeAt": "2026-03-04T05:06:07+01:00"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("temporalQ", &temporal_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (member `temporalQ` is not allowed here), so `temporalQ` is written as text.
    let temporal_q_2: RawJson = r#"{"timerel": "after", "timeAt": "2026-03-04T05:06:07+00:00"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_2.render())?.set_member("temporalQ", &temporal_q_2);
    let request_2 = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
