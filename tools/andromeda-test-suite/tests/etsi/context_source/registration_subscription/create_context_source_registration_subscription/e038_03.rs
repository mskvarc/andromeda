//! ETSI `038_03`: a Context Source Registration Subscription created without `isActive` is active (clause 5.11.2).

use crate::{
    suite_keywords::{expect_registration_subscription_created, ld_json_body_client, registration_subscription_json},
    suite_payloads::building_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can create a context source registration subscription without providing isActive member and
/// will be active by default
/// Clause: 5.11.2
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary holds
/// a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the subscription id
#[gtest]
#[tokio::test]
async fn e038_03_01_create_context_source_registration_subscription_without_is_active_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = expect_registration_subscription_created(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;

    let body = registration_subscription_json(&broker.with_accept(NgsiLdMediaType::Json), &id).await?;
    expect_that!(body.get("status"), some(eq(&json!("active"))));

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
