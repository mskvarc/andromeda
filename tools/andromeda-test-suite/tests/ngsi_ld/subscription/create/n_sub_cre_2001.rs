//! A context nested in geo q temporal q or receiver info is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_context,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription refuses a user @context nested in `geoQ`, in `temporalQ`, in a `receiverInfo` pair,
/// and nested as a string, an array or null, and stores nothing
/// Clause: 5.5.7, 5.8.1
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_sub_cre_2001_a_context_nested_in_geo_q_temporal_q_or_receiver_info_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_4_id = namespace.subscription_id(4)?;
    let subscription_5_id = namespace.subscription_id(5)?;
    let subscription_6_id = namespace.subscription_id(6)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid geoquery at geoQ), so `geoQ` is written as text.
    let geo_q: RawJson =
        r#"{"geometry": "Point", "coordinates": [13.4, 52.5], "georel": "near;maxDistance==1000", "@context": {"Device": "https://example.org/Device"}}"#
            .parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("geoQ", &geo_q);
    let request = core_context_body
        .unchecked(&CreateSubscription { subscription: &subscription })?
        .document(&document);
    let answer = core_context_body.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (member `temporalQ` is not allowed here), so `temporalQ` is written as text.
    let temporal_q: RawJson = r#"{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z", "@context": {"Device": "https://example.org/Device"}}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_2.render())?.set_member("temporalQ", &temporal_q);
    let request_2 = core_context_body
        .unchecked(&CreateSubscription { subscription: &subscription_2 })?
        .document(&document_2);
    let answer_2 = core_context_body.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let endpoint = sink.endpoint("s1")?;
    // The model refuses this subscription, so it is sent as written.
    let request_3 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-3\",\n    \"type\": \"Subscription\",\n    \"entities\": [ {{ \"type\": \"Device_{namespace}\" }} ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{endpoint}\",\n            \"accept\": \"application/json\",\n            \"receiverInfo\": [ {{ \"key\": \"X-Token\", \"value\": \"abc\", \"@context\": {{}} }} ]\n        }}\n    }},\n    \"@context\": \"{core}\"\n}}")));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let endpoint_2 = sink.endpoint("s1")?;
    // The model refuses this subscription, so it is sent as written.
    let request_4 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-4\",\n    \"type\": \"Subscription\",\n    \"entities\": [ {{ \"type\": \"Device_{namespace}\", \"@context\": \"{core}\" }} ],\n    \"notification\": {{ \"endpoint\": {{ \"uri\": \"{endpoint_2}\", \"accept\": \"application/json\" }} }},\n    \"@context\": \"{core}\"\n}}")));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let endpoint_3 = sink.endpoint("s1")?;
    // The model refuses this subscription, so it is sent as written.
    let request_5 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-5\",\n    \"type\": \"Subscription\",\n    \"entities\": [ {{ \"type\": \"Device_{namespace}\", \"@context\": [ \"{core}\", {{ \"a\": \"https://example.org/a\" }} ] }} ],\n    \"notification\": {{ \"endpoint\": {{ \"uri\": \"{endpoint_3}\", \"accept\": \"application/json\" }} }},\n    \"@context\": \"{core}\"\n}}")));
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let endpoint_4 = sink.endpoint("s1")?;
    // The model refuses this subscription, so it is sent as written.
    let request_6 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-6\",\n    \"type\": \"Subscription\",\n    \"entities\": [ {{ \"type\": \"Device_{namespace}\", \"@context\": null }} ],\n    \"notification\": {{ \"endpoint\": {{ \"uri\": \"{endpoint_4}\", \"accept\": \"application/json\" }} }},\n    \"@context\": \"{core}\"\n}}")));
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_7 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_7, answered(StatusCode::NOT_FOUND));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let answer_9 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_3_id)
        .send()
        .await;
    expect_that!(&answer_9, answered(StatusCode::NOT_FOUND));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_4_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    let answer_11 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_5_id)
        .send()
        .await;
    expect_that!(&answer_11, answered(StatusCode::NOT_FOUND));
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_6_id)
        .send()
        .await;
    expect_that!(&answer_12, answered(StatusCode::NOT_FOUND));
    Ok(())
}
