//! ETSI `046_21_02`: deleting an entity notifies a keyValues subscription with the `entityDeleted` trigger and
//! `sysAttrs`, with the deleted entity's identifier, `createdAt` and `deletedAt` (clause 5.8.6).

use crate::context_information::subscription::subscription_notification_behaviour::deletion_case::entity_deletion_notification;
use andromeda_test_suite::{broker, namespace::Namespace, robot_literal, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::subscription::{SubscriptionId, SystemAttributeReporting};
use serde_json::{Map, Value, json};

/// Pins: Delete an entity and check the received notification
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The `length` checks require `$.data` to be an array and its first element an object, which a notification
/// always carries; the Hurl lines also accept a string or another container of that length
#[gtest]
#[tokio::test]
async fn e046_21_02_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let (notification, _subscription) = entity_deletion_notification(&namespace, &id, SystemAttributeReporting::Include).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0").and_then(Value::as_object).map(Map::len), some(eq(5)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(namespace.urn_text("Building", "None")))));
    expect_that!(body.pointer("/data/0/createdAt"), some(anything()));
    expect_that!(body.pointer("/data/0/deletedAt"), some(anything()));

    broker::front()?.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
