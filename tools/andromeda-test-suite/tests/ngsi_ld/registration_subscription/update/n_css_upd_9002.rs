//! A malformed json fragment is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Context Source Registration Subscription with the Content-Type application/json and the
/// unterminated body {"description": "unterminated answers 400 `InvalidRequest`
/// Clause: 5.11.3.4, 5.5.2, 6.3.4
#[gtest]
#[tokio::test]
async fn n_css_upd_9002_a_malformed_json_fragment_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_malformed_id = namespace.subscription_id("malformed")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_malformed = Subscription::registration_builder()
        .id(subscription_malformed_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("malformed")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_malformed).send().await?;
    // The body is not JSON, which the model cannot write, so it is sent as written.
    let request = request_to(
        &broker,
        Method::PATCH,
        &format!("csourceSubscriptions/urn:ngsi-ld:Subscription:{namespace}-malformed"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from("{\"description\": \"unterminated"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    broker.registration_subscriptions().delete(&subscription_malformed_id).send().await?;
    Ok(())
}
