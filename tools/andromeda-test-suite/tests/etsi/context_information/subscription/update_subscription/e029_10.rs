//! An update that sets an `expiresAt` in the future on an expired subscription activates it (clause 5.8.2).

use crate::{batch_operations::expect_batch_done, entity_operations::ld_json_body_client, subscription_operations::create_timed_building_subscription};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary, wire::json_body};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    model::DateTime,
    subscription::{SubscriptionFragment, SubscriptionId},
};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Pins: Check that one can update a subscription: If only expiresAt is included and refers to a `DateTime` in the
/// future, then status shall be updated to "active", if and only if the previous value of status was "expired"
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_10_01_activate_expired_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &sink.endpoint(1)?).await?;

    let expires_in_five_seconds: DateTime = (Utc::now() + TimeDelta::seconds(5)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let expiring = SubscriptionFragment::builder().expires_at(expires_in_five_seconds).build()?;
    broker.subscriptions().update(&id, &expiring).send().await.any_answer();
    sleep(Duration::from_secs(10)).await;

    let activating = SubscriptionFragment::builder()
        .expires_at("2030-08-01T22:07:00Z".parse::<DateTime>()?)
        .build()?;
    expect_batch_done(broker.with_responses().subscriptions().update(&id, &activating).send().await)?;

    let received = broker.with_responses().subscriptions().retrieve(&id).send().await?;
    let body = json_body(&received)?;
    expect_that!(body.get("status"), some(eq(&json!("active"))));

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
