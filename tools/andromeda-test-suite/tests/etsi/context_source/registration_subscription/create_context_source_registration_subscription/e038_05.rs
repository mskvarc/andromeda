//! A Context Source Registration Subscription created with an `expiresAt` is expired once it is due (clause 5.11.2).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::{expect_registration_subscription_created, registration_subscription_json},
    subscription_payloads::building_registration_subscription,
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
