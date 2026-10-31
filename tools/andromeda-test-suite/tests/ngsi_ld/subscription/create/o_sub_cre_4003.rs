//! A non integer join level is bad request data and stores nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create Subscription with a notification `joinLevel` that is not an integer (1.5, the string abc) is
/// refused with `BadRequestData` and stores nothing
/// Clause: 5.2.12, 5.8.1.4, 4.5.23, 5.5.2, 5.5.4
/// Recorded: clause 5.5.4 makes a JSON-LD payload whose data "is not syntactically correct, according to the
/// @context or the API data type definitions" `BadRequestData`, so a `joinLevel` member that is no positive integer
/// is `BadRequestData` however it is wrong; only the URL parameter, outside clause 5.5.4, is `InvalidRequest` when
/// it is no integer (Table 5.5.2-1)
#[gtest]
#[tokio::test]
async fn o_sub_cre_4003_a_non_integer_join_level_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let endpoint = sink.endpoint(1)?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"format\": \"normalized\",\n        \"join\": \"inline\",\n        \"joinLevel\": 1.5,\n        \"endpoint\": {{\n            \"uri\": \"{endpoint}\",\n            \"accept\": \"application/json\"\n        }}\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let endpoint_2 = sink.endpoint(2)?;
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-2\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"format\": \"normalized\",\n        \"join\": \"flat\",\n        \"joinLevel\": \"abc\",\n        \"endpoint\": {{\n            \"uri\": \"{endpoint_2}\",\n            \"accept\": \"application/json\"\n        }}\n    }}\n}}")));
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
