//! An update whose body is not a JSON document is refused with 400 `InvalidRequest` (clause 5.11.3).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::{registration_notification_fragment, registration_subscription_request, update_registration_subscription_request},
    subscription_payloads::{building_registration_subscription, key_values_document},
    unchecked_requests::with_doubled_separator,
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::ErrorType,
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that one cannot update a context source registration subscription with an invalid request body
/// (invalid JSON document)
/// Clause: 5.11.3
#[gtest]
#[tokio::test]
async fn e039_05_01_update_context_source_registration_subscription_with_invalid_json_fragment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).registration_subscription_of(&subscription);
    let created = registration_subscription_request(&client, &subscription, &key_values_document(&subscription, &uri)?)?;
    client.send_unchecked(created).await.any_answer();

    let document = registration_notification_fragment(&sink.endpoint(2)?)?;
    let request = with_doubled_separator(update_registration_subscription_request(&broker, &id, &document)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_distinct_detail());

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
