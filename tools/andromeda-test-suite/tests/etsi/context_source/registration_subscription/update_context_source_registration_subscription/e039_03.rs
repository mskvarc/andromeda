//! An update of a subscription identifier that no Context Source Registration Subscription holds is refused with 404
//! `ResourceNotFound` (clause 5.11.3).

use crate::registration_subscription_operations::{registration_notification_fragment, update_registration_subscription_request};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{model::ErrorType, subscription::SubscriptionId};

/// Pins: Check that one cannot update an unknown context source registration subscription
/// Clause: 5.11.3
#[gtest]
#[tokio::test]
async fn e039_03_01_update_unknown_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &[]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "unknowSubscription").parse()?;
    let linked = broker.with_context(vocabulary.context())?;
    let document = registration_notification_fragment(&sink.endpoint(2)?)?;
    let answer = linked.send_unchecked(update_registration_subscription_request(&linked, &id, &document)?).await;
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_distinct_detail())
}
