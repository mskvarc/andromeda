//! ETSI `046_34`: a subscription with `showChanges` and the `attributeDeleted` trigger notifies a deleted Property,
//! Relationship, `GeoProperty` or `LanguageProperty` as the NGSI-LD Null with its previous value (clauses 5.8.6 and
//! 4.5.18).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription, street_building_document},
        deletion_case::NGSI_LD_NULL,
        notification_case::{data_item, teardown},
    },
    suite_payloads::{BUILDING_LOCATED_STREET_TERMS, building_located_street_geo},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::subscription::{NotificationTrigger, SubscriptionId};
use serde_json::{Value, json};

/// Pins: Delete an attribute from entity and check the notification contains the previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_34_01_show_changes_for_deletion_of_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_34")?)?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::AttributeDeleted)),
        EntityChange::DeleteAttribute(&"name".parse()?),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["name"] = json!({"type": "Property", "value": NGSI_LD_NULL, "previousValue": "Eiffel Tower"});
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Delete an attribute from entity and check the notification contains the previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_34_02_show_changes_for_deletion_of_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_34")?)?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::AttributeDeleted)),
        EntityChange::DeleteAttribute(&"locatedAt".parse()?),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["locatedAt"] = json!({
        "type": "Relationship",
        "previousObject": namespace.urn_text("City", "Paris"),
        "object": NGSI_LD_NULL,
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Delete an attribute from entity and check the notification contains the previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_34_03_show_changes_for_deletion_of_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_34")?)?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::AttributeDeleted)),
        EntityChange::DeleteAttribute(&"location".parse()?),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["location"] = json!({
        "type": "GeoProperty",
        "value": NGSI_LD_NULL,
        "previousValue": {"type": "Point", "coordinates": [-8.5, 41.2]},
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Delete an attribute from entity and check the notification contains the previous value
/// Clause: 5.8.6, 4.5.18
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_34_04_show_changes_for_deletion_of_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_34")?)?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::AttributeDeleted)),
        EntityChange::DeleteAttribute(&"street".parse()?),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["street"] = json!({
        "type": "LanguageProperty",
        "languageMap": {"@none": NGSI_LD_NULL},
        "previousLanguageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
