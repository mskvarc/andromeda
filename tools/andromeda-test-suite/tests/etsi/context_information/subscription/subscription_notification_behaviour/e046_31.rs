//! ETSI `046_31`: a subscription with `showChanges` notifies an updated Property, Relationship, `GeoProperty` or
//! `LanguageProperty` with its previous value beside the current one (clauses 5.8.6 and 4.5.18).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription, street_building_document},
        notification_case::{data_item, teardown},
    },
    suite_payloads::{BUILDING_LOCATED_STREET_TERMS, building_located_street_geo, point},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityFragment, Relationship},
    subscription::SubscriptionId,
};
use serde_json::{Value, json};

/// Pins: Update an attribute in entity and check the notification contains the previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_31_01_show_changes_for_an_update_of_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_31")?)?;
    let fragment = EntityFragment::builder().property("name", "Tour Eiffel").build()?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, None),
        EntityChange::UpdateAttributes(&fragment, ContextForm::List),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["name"] = json!({"type": "Property", "value": "Tour Eiffel", "previousValue": "Eiffel Tower"});
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Update an attribute in entity and check the notification contains the previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_31_02_show_changes_for_an_update_of_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_31")?)?;
    let fragment = EntityFragment::builder()
        .relationship("locatedAt", Relationship::builder(namespace.urn("City", "Nantes")?))
        .build()?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, None),
        EntityChange::UpdateAttributes(&fragment, ContextForm::List),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["locatedAt"] = json!({
        "type": "Relationship",
        "previousObject": namespace.urn_text("City", "Paris"),
        "object": namespace.urn_text("City", "Nantes"),
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Update an attribute in entity and check the notification contains the previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_31_03_show_changes_for_an_update_of_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_31")?)?;
    let fragment = EntityFragment::builder().geo_property("location", point(13.3986, 52.5547)?).build()?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, None),
        EntityChange::UpdateAttributes(&fragment, ContextForm::List),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["location"] = json!({
        "type": "GeoProperty",
        "value": {"type": "Point", "coordinates": [13.3986, 52.5547]},
        "previousValue": {"type": "Point", "coordinates": [-8.5, 41.2]},
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Update an attribute in entity and check the notification contains the previous value
/// Clause: 5.8.6, 4.5.18
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_31_04_show_changes_for_an_update_of_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_31")?)?;
    let fragment = EntityFragment::builder()
        .language_property("street", [("fr", "Grand Place"), ("es", "Gran Lugar")])
        .build()?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, None),
        EntityChange::UpdateAttributes(&fragment, ContextForm::List),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let mut expected = street_building_document(&namespace);
    expected["street"] = json!({
        "type": "LanguageProperty",
        "languageMap": {"fr": "Grand Place", "es": "Gran Lugar"},
        "previousLanguageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
    });
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
