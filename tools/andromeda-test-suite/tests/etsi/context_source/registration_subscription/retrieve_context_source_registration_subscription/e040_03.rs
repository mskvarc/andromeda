//! ETSI `040_03`: a retrieve of a subscription identifier that no Context Source Registration Subscription holds is
//! refused with 404 `ResourceNotFound` (clause 5.11.4).

use crate::suite_keywords::problem_content_type;
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{ErrorType, NgsiLdMediaType},
    subscription::SubscriptionId,
};

/// Pins: Check that one cannot retrieve an unknown context source registration subscription, an error of type
/// `ResourceNotFound` shall be raised
/// Clause: 5.11.4
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112 clause
/// 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code the
/// phrase names
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e040_03_01_retrieve_unknown_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "unknowSubscription").parse()?;
    let answer = broker
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .registration_subscriptions()
        .retrieve(&id)
        .send()
        .await;
    expect_that!(problem_content_type(&answer), some(eq("application/json")));
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_distinct_detail())
}
