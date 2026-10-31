//! An unbalanced pick group is refused.

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

/// Pins: Create Subscription with pick `["owner{"]` answers 400 `BadRequestData` and creates nothing
/// Clause: Table 5.2.14.1-1, 4.21, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9008_an_unbalanced_pick_group_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_r7_id = namespace.subscription_id("r7")?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-r7\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{unreachable}/notify\",\n            \"accept\": \"application/json\"\n        }},\n        \"pick\": [\n            \"owner{{\"\n        ]\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_r7_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
