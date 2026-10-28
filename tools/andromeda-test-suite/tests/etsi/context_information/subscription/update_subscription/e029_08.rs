//! ETSI `029_08`: an update that sets `isActive` to true with an `expiresAt` in the future activates a paused
//! subscription (clause 5.8.2).

use crate::suite_keywords::{create_paused_building_subscription, expect_batch_done, ld_json_body_client};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use ngsi_ld::{
    model::DateTime,
    subscription::{Activation, SubscriptionFragment, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can update a subscription: If isActive is equal to true and expiresAt corresponds to a
/// `DateTime` in the future, then status shall be updated to "active"
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_08_01_active_true_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_paused_building_subscription(&client, &id, &sink.endpoint(1)?).await?;

    let fragment = SubscriptionFragment::builder()
        .activation(Activation::Active)
        .expires_at("2030-08-01T22:07:00Z".parse::<DateTime>()?)
        .build()?;
    expect_batch_done(broker.with_responses().subscriptions().update(&id, &fragment).send().await)?;

    let received = broker.with_responses().subscriptions().retrieve(&id).send().await?;
    let body = json_body(&received)?;
    expect_that!(body.get("status"), some(eq(&json!("active"))));

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
