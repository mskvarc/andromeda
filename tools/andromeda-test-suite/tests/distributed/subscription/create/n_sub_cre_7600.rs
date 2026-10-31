//! A subscription without notification is bad request data and stores nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{http::headers::Accept, model::ErrorType};

/// Pins: Create Subscription on broker A without a notification member answers 400 `BadRequestData`, and no such
/// subscription is found
/// Clause: 5.8.1.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_7600_a_subscription_without_notification_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subscription_id = namespace.subscription_id(1)?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ]\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
