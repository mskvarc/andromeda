//! ETSI `038_05`: a Context Source Registration Subscription created with an `expiresAt` is expired once it is due
//! (clause 5.11.2).

use crate::{
    suite_keywords::{expect_registration_subscription_created, ld_json_body_client, registration_subscription_json},
    suite_payloads::building_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Pins: Check that one can create a context source registration subscription with an expiresAt member and when it is
/// due the status of the subscription changes to "expired"
/// Clause: 5.11.2
/// Corrected: `fix(038_05)`: import the datetime library the case relies on
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary holds
/// a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the subscription id
#[gtest]
#[tokio::test]
async fn e038_05_01_create_context_source_registration_subscription_with_expires_at_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let expires_at = (Utc::now() + TimeDelta::seconds(10)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, Some(expires_at.parse()?))?;
    let _subscription = expect_registration_subscription_created(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    sleep(Duration::from_secs(15)).await;

    let body = registration_subscription_json(&broker.with_accept(NgsiLdMediaType::Json), &id).await?;
    expect_that!(body.get("expiresAt"), some(eq(&json!(expires_at))));
    expect_that!(body.get("status"), some(eq(&json!("expired"))));

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
