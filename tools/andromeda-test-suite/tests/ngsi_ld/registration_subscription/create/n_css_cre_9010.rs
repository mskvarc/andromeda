//! A receiver info entry that is not an http header is bad request data and stores nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription with an http endpoint whose `receiverInfo` has a key with
/// a space or a value with a line break answers 400 `BadRequestData` and stores nothing, while a good
/// `receiverInfo` answers 201
/// Clause: 5.2.15, 6.3.9, 5.11.2.4, Table 5.2.22-1
#[gtest]
#[tokio::test]
async fn n_css_cre_9010_a_receiver_info_entry_that_is_not_an_http_header_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Bad Name", "x")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    let answer = broker.with_responses().registration_subscriptions().create(&subscription).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(2)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("X-A", "a\nb")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    let answer_3 = broker.with_responses().registration_subscriptions().create(&subscription_2).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_3 = Subscription::registration_builder()
        .id(subscription_3_id)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(3)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Authorization", "Bearer abc")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription_3).send().await?;
    Ok(())
}
