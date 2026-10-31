//! An `entityCreated` subscription notifies the creation of an entity of its selection, in keyValues format (clause
//! 5.8.6).

use crate::{
    building_payloads::eiffel_tower_located,
    context_information::subscription::subscription_notification_behaviour::notification_case::teardown,
    entity_operations::{ld_json_body_client, send_create_entity},
    subscription_operations::{create_subscription_document, simplified_notification},
    subscription_payloads::key_values_document,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    query::EntitySelector,
    subscription::{NotificationTrigger, Subscription, SubscriptionId},
};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Pins: Check that a notification is sent when an entity is created and entityCreated notification trigger is configured
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_06_01_check_that_a_notification_is_sent_with_all_matching_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory"]).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let sink = NotificationSink::start().await?;
    let endpoint_url = sink.endpoint(1)?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(simplified_notification(&endpoint_url)?)
        .build()?;
    let _subscription = create_subscription_document(&client, &subscription, &key_values_document(&subscription, &endpoint_url)?).await?;
    sleep(Duration::from_secs(1)).await;

    let building = eiffel_tower_located(&namespace, 1)?;
    send_create_entity(&client, &building).await.any_answer();

    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));

    teardown(&broker, &id, &building).await;
    Ok(())
}
