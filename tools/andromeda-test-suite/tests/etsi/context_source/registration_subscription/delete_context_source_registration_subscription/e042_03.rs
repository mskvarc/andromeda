//! A delete of a subscription identifier that no Context Source Registration Subscription holds is refused with 404
//! `ResourceNotFound` (clause 5.11.6).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{model::ErrorType, subscription::SubscriptionId};

/// Pins: Check that one cannot delete an unknown context source registration subscription
/// Clause: 5.11.6
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e042_03_01_delete_unknown_context_source_registration_subscription_with_invalid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "unknowSubscription").parse()?;
    let answer = broker.with_responses().registration_subscriptions().delete(&id).send().await;
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title())
}
