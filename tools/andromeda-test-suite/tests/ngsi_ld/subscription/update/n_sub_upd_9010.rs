//! An update with a receiver info entry that is not an http header is bad request data and keeps the stored one.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, nth::nth, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, KeyValuePair, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with an http endpoint whose `receiverInfo` has a key with a space, a key with a colon
/// or a value with a line break answers 400 `BadRequestData` and keeps the stored `receiverInfo`
/// Clause: 5.2.15, 6.3.8, 5.8.2.4, Table 5.2.22-1
#[gtest]
#[tokio::test]
async fn n_sub_upd_9010_an_update_with_a_receiver_info_entry_that_is_not_an_http_header_is_bad_request_data_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let receiver_type = namespace.entity_type("Receiver")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([receiver_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Authorization", "Bearer abc")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder()
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
    let answer = broker.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_2
            .notification()
            .endpoint()
            .receiver_info()
            .iter()
            .map(KeyValuePair::key)
            .collect::<Vec<_>>(),
        nth(0, eq(&"Authorization"))
    );
    expect_that!(
        subscription_2
            .notification()
            .endpoint()
            .receiver_info()
            .iter()
            .map(KeyValuePair::value)
            .collect::<Vec<_>>(),
        nth(0, eq(&"Bearer abc"))
    );
    let fragment_2 = SubscriptionFragment::builder()
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Bad:Name", "x")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    let answer_2 = broker.with_responses().subscriptions().update(&subscription_id, &fragment_2).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_3
            .notification()
            .endpoint()
            .receiver_info()
            .iter()
            .map(KeyValuePair::key)
            .collect::<Vec<_>>(),
        nth(0, eq(&"Authorization"))
    );
    expect_that!(
        subscription_3
            .notification()
            .endpoint()
            .receiver_info()
            .iter()
            .map(KeyValuePair::value)
            .collect::<Vec<_>>(),
        nth(0, eq(&"Bearer abc"))
    );
    let fragment_3 = SubscriptionFragment::builder()
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("X-A", "a\nb")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    let answer_3 = broker.with_responses().subscriptions().update(&subscription_id, &fragment_3).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_4
            .notification()
            .endpoint()
            .receiver_info()
            .iter()
            .map(KeyValuePair::key)
            .collect::<Vec<_>>(),
        nth(0, eq(&"Authorization"))
    );
    expect_that!(
        subscription_4
            .notification()
            .endpoint()
            .receiver_info()
            .iter()
            .map(KeyValuePair::value)
            .collect::<Vec<_>>(),
        nth(0, eq(&"Bearer abc"))
    );
    Ok(())
}
