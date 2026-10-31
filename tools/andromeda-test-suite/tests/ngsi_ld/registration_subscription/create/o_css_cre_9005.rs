//! Attributes beside pick or omit in a context source notification is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Create Context Source Registration Subscription with a notification holding attributes beside pick or
/// beside omit answers 400 `BadRequestData`
/// Clause: 4.21, 5.2.14, 5.11.2.3
/// Recorded: no clause forbids combining pick or omit with the deprecated attrs, which Tables 6.4.3.2-1, 6.5.3.1-1
/// and 5.2.14.1-1 make a synonym for pick (attributes in a notification); Andromeda refuses attrs beside pick or
/// omit with `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_css_cre_9005_attributes_beside_pick_or_omit_in_a_context_source_notification_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "csourceSubscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-csource-0\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{unreachable}/notify\",\n            \"accept\": \"application/json\"\n        }},\n        \"pick\": [\n            \"id\"\n        ],\n        \"attributes\": [\n            \"id\"\n        ]\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "csourceSubscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-csource-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{unreachable}/notify\",\n            \"accept\": \"application/json\"\n        }},\n        \"omit\": [\n            \"id\"\n        ],\n        \"attributes\": [\n            \"id\"\n        ]\n    }}\n}}")));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
