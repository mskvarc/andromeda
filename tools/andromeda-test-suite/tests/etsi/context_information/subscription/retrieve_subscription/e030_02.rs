//! ETSI `030_02`: a retrieve of a subscription identifier that no subscription holds is refused with 404
//! `ResourceNotFound` (clause 5.8.3).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{model::ErrorType, subscription::SubscriptionId};

/// Pins: Check that one cannot retrieve a subscription: If the identifier provided does not correspond to any existing
/// subscription in the system then an error of type `ResourceNotFound` shall be raised
/// Clause: 5.8.3
#[gtest]
#[tokio::test]
async fn e030_02_01_retrieve_unknown_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "unknowSubscription").parse()?;
    let answer = broker.with_responses().subscriptions().retrieve(&id).send().await;
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title())
}
