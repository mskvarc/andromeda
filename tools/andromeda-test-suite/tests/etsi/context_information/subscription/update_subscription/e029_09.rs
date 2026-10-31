//! An update that sets `isActive` to false pauses a subscription, whose status then reads `paused` (clause 5.8.2).

use crate::{batch_operations::expect_batch_done, entity_operations::ld_json_body_client, subscription_operations::create_timed_building_subscription};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use ngsi_ld::subscription::{Activation, SubscriptionFragment, SubscriptionId};
use serde_json::json;

/// Pins: Check that one can update a subscription: If isActive is equal to false and expiresAt is not present, then
/// status shall be updated to "paused", if and only if, the previous value of status was different than "expired"
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_09_01_update_subscription_status_to_paused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &sink.endpoint(1)?).await?;

    let fragment = SubscriptionFragment::builder().activation(Activation::Paused).build()?;
    expect_batch_done(broker.with_responses().subscriptions().update(&id, &fragment).send().await)?;

    let received = broker.with_responses().subscriptions().retrieve(&id).send().await?;
    let body = json_body(&received)?;
    expect_that!(body.get("status"), some(eq(&json!("paused"))));

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
