//! A bad id or fragment is bad request data and an unknown id not found.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{UncheckedDocument, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Context Source Registration Subscription on broker A of an id that is not a URI, or with a fragment
/// holding an empty `watchedAttributes`, answers 400 `BadRequestData`, and of an unknown id 404 `ResourceNotFound`
/// Clause: 5.11.3.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_upd_7601_a_bad_id_or_fragment_is_bad_request_data_and_an_unknown_id_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    // The model refuses the id this request names, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, "csourceSubscriptions/not-a-uri")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"description\": \"x\"\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment = SubscriptionFragment::registration_builder().build()?;
    // The model refuses it (the member cannot be empty at watchedAttributes), so `watchedAttributes` is written as text.
    let watched_attributes: RawJson = "[]".parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("watchedAttributes", &watched_attributes);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = SubscriptionFragment::registration_builder().description("x").build()?;
    let answer_3 = broker
        .with_responses()
        .registration_subscriptions()
        .update(&subscription_2_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
