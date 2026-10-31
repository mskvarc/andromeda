//! A type selection outside the grammar in an entity selector is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::{EntitySelector, SelectorType, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription refuses an entities selector whose type is a type selection outside the 4.17 grammar
/// with `BadRequestData` and stores no subscription, and accepts the parenthesised conjunction
/// Clause: 4.17, 5.2.33, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_3040_a_type_selection_outside_the_grammar_in_an_entity_selector_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_4_id = namespace.subscription_id(4)?;
    // Table 5.2.33-1: type is "A valid type selection string as per clause 4.17"
    let endpoint = sink.endpoint(1)?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"id\": \"urn:ngsi-ld:Subscription:{namespace}-1\",\n  \"type\": \"Subscription\",\n  \"entities\": [ {{ \"type\": \"Home_{namespace};Vehicle_{namespace}\" }} ],\n  \"notification\": {{\n    \"endpoint\": {{ \"uri\": \"{endpoint}\", \"accept\": \"application/json\" }}\n  }}\n}}")));
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
    let endpoint_2 = sink.endpoint(2)?;
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"id\": \"urn:ngsi-ld:Subscription:{namespace}-2\",\n  \"type\": \"Subscription\",\n  \"entities\": [ {{ \"type\": \"((Home_{namespace};Vehicle_{namespace}))\" }} ],\n  \"notification\": {{\n    \"endpoint\": {{ \"uri\": \"{endpoint_2}\", \"accept\": \"application/json\" }}\n  }}\n}}")));
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let endpoint_3 = sink.endpoint(3)?;
    // The model refuses this subscription, so it is sent as written.
    let request_3 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"id\": \"urn:ngsi-ld:Subscription:{namespace}-3\",\n  \"type\": \"Subscription\",\n  \"entities\": [ {{ \"type\": \"(Home_{namespace}|Vehicle_{namespace})\" }} ],\n  \"notification\": {{\n    \"endpoint\": {{ \"uri\": \"{endpoint_3}\", \"accept\": \"application/json\" }}\n  }}\n}}")));
    let answer_4 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // control: the parenthesised conjunction is stored
    let subscription_4 = Subscription::builder()
        .id(subscription_4_id)
        .entities([EntitySelector::new(SelectorType::Selection(
            type_selection::all_of([&namespace.entity_type("Home")?, &namespace.entity_type("Vehicle")?]).build()?,
        ))])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(4)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_4).send().await?;
    Ok(())
}
