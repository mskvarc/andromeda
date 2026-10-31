//! A boolean after an ordering operator in a subscription q is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription refuses a q whose ordering operator is followed by true or false with `BadRequestData`
/// and stores no subscription, and accepts the Boolean after == and !=
/// Clause: 4.9, 5.2.12, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_3050_a_boolean_after_an_ordering_operator_in_a_subscription_q_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_4_id = namespace.subscription_id(4)?;
    let subscription_5_id = namespace.subscription_id(5)?;
    let subscription_6_id = namespace.subscription_id(6)?;
    // Table 5.2.12-1: q is "A valid query string as per clause 4.9", and ComparableValue excludes true and false
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q: RawJson = r#""b>true""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("q", &q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([sensor_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q_2: RawJson = r#""b<=false""#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_2.render())?.set_member("q", &q_2);
    let request_2 = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id)
        .entities([sensor_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q_3: RawJson = r#""b>=true""#.parse()?;
    let document_3 = UncheckedDocument::try_from(subscription_3.render())?.set_member("q", &q_3);
    let request_3 = broker.unchecked(&CreateSubscription { subscription: &subscription_3 })?.document(&document_3);
    let answer_4 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let subscription_4 = Subscription::builder()
        .id(subscription_4_id)
        .entities([sensor_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(4)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q_4: RawJson = r#""b<false""#.parse()?;
    let document_4 = UncheckedDocument::try_from(subscription_4.render())?.set_member("q", &q_4);
    let request_4 = broker.unchecked(&CreateSubscription { subscription: &subscription_4 })?.document(&document_4);
    let answer_5 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the Boolean after == and != is stored
    let subscription_5 = Subscription::builder()
        .id(subscription_5_id)
        .entities([sensor_type.as_str()])
        .q(q::attr("b").eq(true).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(5)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_5).send().await?;
    let subscription_6 = Subscription::builder()
        .id(subscription_6_id)
        .entities([sensor_type.as_str()])
        .q(q::attr("b").ne(false).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(6)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_6).send().await?;
    Ok(())
}
