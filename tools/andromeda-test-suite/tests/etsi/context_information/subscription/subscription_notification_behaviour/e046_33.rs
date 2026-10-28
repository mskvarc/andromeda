//! ETSI `046_33`: a subscription with `showChanges` notifies an updated `VocabProperty` with its previous `vocab`
//! (clauses 5.8.6 and 4.5.20).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription},
        notification_case::{data_item, teardown},
    },
    suite_payloads::building_with_vocab_property,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityFragment, VocabProperty, VocabTerm},
    subscription::SubscriptionId,
};
use serde_json::{Value, json};

/// Pins: Update the `VocabProperty` in the entity and check the notification contains the previous value
/// Clause: 5.8.6, 4.5.20
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_33_01_update_vocab_property_and_check_previous_value_in_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_with_vocab_property(&namespace, "046_33")?;
    let fragment = EntityFragment::builder()
        .vocab_property("vocabProperty", VocabProperty::builder("museum".parse::<VocabTerm>()?))
        .build()?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &["Building", "vocabProperty"],
        &[&building],
        ContextForm::List,
        |uri| show_changes_subscription(&id, uri, None),
        EntityChange::UpdateAttributes(&fragment, ContextForm::String),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let expected = json!({
        "type": "Building",
        "vocabProperty": {"type": "VocabProperty", "vocab": "museum", "previousVocab": "monument"},
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
