//! A subscription with `showChanges` and the `attributeDeleted` trigger notifies a deleted `VocabProperty` as the
//! NGSI-LD Null with its previous `vocab` (clauses 5.8.6 and 4.5.20).

use crate::{
    building_payloads::building_with_vocab_property,
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription},
        deletion_case::NGSI_LD_NULL,
        notification_case::{data_item, teardown},
    },
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::subscription::{NotificationTrigger, SubscriptionId};
use serde_json::{Value, json};

/// Pins: Delete a `VocabProperty` from entity and check the notification contains the previous value
/// Clause: 5.8.6, 4.5.20
#[gtest]
#[tokio::test]
async fn e046_36_01_delete_attribute_and_check_previous_value_in_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_with_vocab_property(&namespace, "046_36")?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &["Building", "vocabProperty"],
        &[&building],
        ContextForm::List,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::AttributeDeleted)),
        EntityChange::DeleteAttribute(&"vocabProperty".parse()?),
    )
    .await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let expected = json!({
        "type": "Building",
        "vocabProperty": {"type": "VocabProperty", "vocab": NGSI_LD_NULL.as_str(), "previousVocab": "monument"},
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
