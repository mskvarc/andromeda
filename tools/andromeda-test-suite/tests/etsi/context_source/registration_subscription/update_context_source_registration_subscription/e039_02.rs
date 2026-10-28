//! ETSI `039_02`: an update whose target is not a valid URI is refused with 400 `BadRequestData` (clause 5.11.3).

use crate::suite_keywords::{registration_notification_fragment, update_registration_subscription_request};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, vocabulary::Vocabulary, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{model::ErrorType, subscription::SubscriptionId};

/// Pins: Check that one cannot update a context source registration subscription with an invalid URI
/// Clause: 5.11.3
#[gtest]
#[tokio::test]
async fn e039_02_01_update_context_source_registration_subscription_with_invalid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &[]).await?;
    let sink = NotificationSink::start().await?;
    let linked = broker.with_context(vocabulary.context())?;
    let document = registration_notification_fragment(&sink.endpoint(2)?)?;
    // The typed operation takes only a valid identifier, so its target is replaced by the invalid one the case sends.
    let placeholder: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let request = update_registration_subscription_request(&linked, &placeholder, &document)?.uri(api_uri(&linked, "csourceSubscriptions/invalidUri")?);
    let answer = linked.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail())
}
