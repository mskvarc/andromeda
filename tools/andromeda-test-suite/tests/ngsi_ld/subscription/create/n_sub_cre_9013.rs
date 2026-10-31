//! A cooldown or timeout that is not a positive number is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
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

/// Pins: Create Subscription with an endpoint cooldown of 0 or "abc", or a timeout of -1 or 0, answers 400
/// `BadRequestData` and stores nothing
/// Clause: Table 5.2.15-1, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9013_a_cooldown_or_timeout_that_is_not_a_positive_number_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_refused_0_id = namespace.subscription_id("refused-0")?;
    let subscription_refused_1_id = namespace.subscription_id("refused-1")?;
    let subscription_refused_2_id = namespace.subscription_id("refused-2")?;
    let subscription_refused_3_id = namespace.subscription_id("refused-3")?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-refused-0\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"id\": \"urn:ngsi-ld:Building:{namespace}-refused\",\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{unreachable}/notify\",\n            \"accept\": \"application/json\",\n            \"cooldown\": 0\n        }}\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_refused_0_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-refused-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"id\": \"urn:ngsi-ld:Building:{namespace}-refused\",\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{unreachable}/notify\",\n            \"accept\": \"application/json\",\n            \"cooldown\": \"abc\"\n        }}\n    }}\n}}")));
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_refused_1_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    // The model refuses this subscription, so it is sent as written.
    let request_3 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-refused-2\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"id\": \"urn:ngsi-ld:Building:{namespace}-refused\",\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{unreachable}/notify\",\n            \"accept\": \"application/json\",\n            \"timeout\": -1\n        }}\n    }}\n}}")));
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_refused_2_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    // The model refuses this subscription, so it is sent as written.
    let request_4 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-refused-3\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"id\": \"urn:ngsi-ld:Building:{namespace}-refused\",\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{unreachable}/notify\",\n            \"accept\": \"application/json\",\n            \"timeout\": 0\n        }}\n    }}\n}}")));
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_refused_3_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    Ok(())
}
