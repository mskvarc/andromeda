//! A context nested in entities or notification is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create Context Source Registration Subscription refuses a user @context nested in an entities member and
/// in the notification, and stores nothing
/// Clause: 5.5.7, 5.11.2
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_css_cre_2001_a_context_nested_in_entities_or_notification_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let endpoint = sink.endpoint("s1")?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "csourceSubscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [ {{ \"type\": \"Device_{namespace}\", \"@context\": {{ \"Device\": \"https://example.org/Device\" }} }} ],\n    \"notification\": {{ \"endpoint\": {{ \"uri\": \"{endpoint}\", \"accept\": \"application/json\" }} }},\n    \"@context\": \"{core}\"\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let endpoint_2 = sink.endpoint("s1")?;
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "csourceSubscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-2\",\n    \"type\": \"Subscription\",\n    \"entities\": [ {{ \"type\": \"Device_{namespace}\" }} ],\n    \"notification\": {{\n        \"endpoint\": {{ \"uri\": \"{endpoint_2}\", \"accept\": \"application/json\" }},\n        \"@context\": {{}}\n    }},\n    \"@context\": \"{core}\"\n}}")));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
