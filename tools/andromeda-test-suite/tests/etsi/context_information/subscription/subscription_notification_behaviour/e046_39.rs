//! ETSI `046_39`: a subscription with `showChanges` and the `entityDeleted` trigger notifies a deleted entity with its
//! `VocabProperty` as the NGSI-LD Null beside its previous `vocab` (clauses 5.8.6, 4.5.18 and 4.5.20).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription},
        deletion_case::NGSI_LD_NULL,
        notification_case::{ANY_SYSTEM_TIME, data_item, teardown},
    },
    suite_payloads::building_with_vocab_property,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::subscription::{NotificationTrigger, SubscriptionId};
use serde_json::{Value, json};

/// Pins: Delete an entity and check the notification contains the previous values for a `VocabProperty`
/// Clause: 5.8.6, 4.5.18, 4.5.20
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_39_01_delete_entity_and_check_previous_values_in_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_with_vocab_property(&namespace, "046_39")?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &["Building", "vocabProperty"],
        &[&building],
        ContextForm::List,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::EntityDeleted)),
        EntityChange::DeleteEntity,
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let expected = json!({
        "type": "Building",
        "deletedAt": ANY_SYSTEM_TIME,
        "vocabProperty": {"type": "VocabProperty", "previousVocab": "monument", "vocab": NGSI_LD_NULL.as_str()},
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
