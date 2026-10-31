//! A Context Source Registration Subscription whose `expiresAt` is past is refused with 400 `BadRequestData` (clause
//! 5.11.2).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::registration_subscription_request,
    subscription_payloads::{building_registration_subscription, key_values_document},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::ErrorType,
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that one cannot create a context source registration subscription with an expiration timestamp
/// representing a moment before the current date and time
/// Clause: 5.11.2
#[gtest]
#[tokio::test]
async fn e038_09_01_create_expired_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id), &uri, Activation::Unspecified, Some("2018-08-01T22:07:00Z".parse()?))?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).registration_subscription_of(&subscription);
    let request = registration_subscription_request(&client, &subscription, &key_values_document(&subscription, &uri)?)?;
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail())
}
