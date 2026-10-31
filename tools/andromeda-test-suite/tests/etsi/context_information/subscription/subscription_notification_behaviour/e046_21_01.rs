//! Deleting an entity notifies a keyValues subscription with the `entityDeleted` trigger, with the deleted entity's
//! identifier and `deletedAt` (clause 5.8.6).

use crate::context_information::subscription::subscription_notification_behaviour::deletion_case::entity_deletion_notification;
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::subscription::{SubscriptionId, SystemAttributeReporting};
use serde_json::{Map, Value, json};

/// Pins: Delete an entity and check the received notification
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_21_01_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let (notification, _subscription) = entity_deletion_notification(&namespace, &id, SystemAttributeReporting::Unspecified).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0").and_then(Value::as_object).map(Map::len), some(eq(3)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(namespace.urn_text("Building", "None")))));
    expect_that!(body.pointer("/data/0/deletedAt"), some(anything()));

    broker::front()?.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
