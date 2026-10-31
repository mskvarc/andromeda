//! Invalid subscriptions are bad request data and store nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription on broker A without notification, with type `RandomType`,
/// with a malformed q, with empty `watchedAttributes` or with an `expiresAt` in the past answers 400
/// `BadRequestData` and stores nothing
/// Clause: 5.11.2.4, 5.2.12, 4.9
#[gtest]
#[tokio::test]
async fn n_css_cre_7601_invalid_subscriptions_are_bad_request_data_and_store_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_4_id = namespace.subscription_id(4)?;
    let subscription_5_id = namespace.subscription_id(5)?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "csourceSubscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ]\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let endpoint = sink.endpoint(1)?;
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "csourceSubscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-2\",\n    \"type\": \"RandomType_{namespace}\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{endpoint}\",\n            \"accept\": \"application/json\"\n        }}\n    }}\n}}")));
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registration_subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_3 = Subscription::registration_builder()
        .id(subscription_3_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q: RawJson = r#""(invalidQuery""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_3.render())?.set_member("q", &q);
    let request_3 = broker.unchecked(&CreateSubscription { subscription: &subscription_3 })?.document(&document);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registration_subscriptions()
        .retrieve(&subscription_3_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let subscription_4 = Subscription::registration_builder()
        .id(subscription_4_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (the member cannot be empty at watchedAttributes), so `watchedAttributes` is written as text.
    let watched_attributes: RawJson = "[]".parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_4.render())?.set_member("watchedAttributes", &watched_attributes);
    let request_4 = broker.unchecked(&CreateSubscription { subscription: &subscription_4 })?.document(&document_2);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registration_subscriptions()
        .retrieve(&subscription_4_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let subscription_5 = Subscription::registration_builder()
        .id(subscription_5_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at("2018-08-01T22:07:00Z".parse()?)
        .build()?;
    let answer_9 = broker.with_responses().registration_subscriptions().create(&subscription_5).send().await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registration_subscriptions()
        .retrieve(&subscription_5_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    Ok(())
}
