//! An unknown subscription is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Delete Context Source Registration Subscription of a well-formed id naming no subscription answers 404
/// `ResourceNotFound` with a title
/// Clause: 5.11.6.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_css_del_9001_an_unknown_subscription_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subscription_absent_id = namespace.subscription_id("absent")?;
    let answer = broker
        .with_responses()
        .registration_subscriptions()
        .delete(&subscription_absent_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
