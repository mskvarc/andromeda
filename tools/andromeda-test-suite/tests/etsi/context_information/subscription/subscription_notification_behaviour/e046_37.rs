//! ETSI `046_37`: a subscription with `showChanges` and the `entityDeleted` trigger notifies a deleted entity with each
//! attribute as the NGSI-LD Null beside its previous value (clauses 5.8.6 and 4.5.18).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription},
        deletion_case::NGSI_LD_NULL,
        notification_case::{ANY_SYSTEM_TIME, data_item, teardown},
    },
    suite_payloads::{BUILDING_LOCATED_STREET_TERMS, building_located_street_geo},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::subscription::{NotificationTrigger, SubscriptionId};
use serde_json::{Value, json};

/// Pins: Delete an entity and check the notification contains the previous values for all attributes
/// Clause: 5.8.6, 4.5.18
/// Corrected: fix(`046_37_01`): expect the languageMap of a deleted `LanguageProperty` as an @none null (5.8.6, 4.5.18.2)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_37_01_delete_entity_and_check_previous_values_in_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_37")?)?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
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
        "location": {
            "type": "GeoProperty",
            "previousValue": {"type": "Point", "coordinates": [-8.5, 41.2]},
            "value": NGSI_LD_NULL,
        },
        "name": {"type": "Property", "previousValue": "Eiffel Tower", "value": NGSI_LD_NULL},
        "street": {
            "type": "LanguageProperty",
            "previousLanguageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
            "languageMap": {"@none": NGSI_LD_NULL},
        },
        "locatedAt": {
            "type": "Relationship",
            "previousObject": namespace.urn_text("City", "Paris"),
            "object": NGSI_LD_NULL,
        },
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
