//! A bad id is bad request data and an unknown one not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{model::ErrorType, subscription::SubscriptionFragment};

/// Pins: Update Subscription on broker A of an id that is not a URI answers 400 `BadRequestData`, and of an unknown
/// id 404 `ResourceNotFound`
/// Clause: 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_7600_a_bad_id_is_bad_request_data_and_an_unknown_one_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subscription_id = namespace.subscription_id(1)?;
    // The model refuses the id this request names, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, "subscriptions/not-a-uri")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"description\": \"x\"\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment = SubscriptionFragment::builder().description("x").build()?;
    let answer_2 = broker.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
