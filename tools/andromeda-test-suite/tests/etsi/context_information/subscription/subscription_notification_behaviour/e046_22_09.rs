//! ETSI `046_22_09`: deleting the `name` Property by writing the NGSI-LD Null with Merge Entity, Update Attributes or
//! Partial Attribute Update notifies a normalized subscription watching it with the `attributeDeleted` trigger, the
//! attribute reported as a Property whose value is the NGSI-LD Null (clause 5.8.6).

use crate::context_information::subscription::subscription_notification_behaviour::{
    deletion_case::{NullWrite, StreetBuildingAttribute, street_building_after_deletion, street_building_deletion_notification},
    notification_case::teardown,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::subscription::SubscriptionId;
use serde_json::{Value, json};

/// Pins: Delete an attribute via Merge Entity operation and check the received notification
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The `length` check requires `$.data` to be an array, which a notification always carries; the Hurl line also
/// accepts a string or an object of that length
#[gtest]
#[tokio::test]
async fn e046_22_09_01_check_that_a_notification_is_sent_with_matching_entity_when_deleting_via_merge_entity_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let deleted = StreetBuildingAttribute::Name;
    let (notification, building, _subscription) = street_building_deletion_notification(&namespace, &id, deleted, NullWrite::MergeEntity).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(matches_document(street_building_after_deletion(&namespace, deleted)).hiding_everywhere("id"))
    );

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Delete an attribute via Update Attributes operation and check the received notification
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The `length` check requires `$.data` to be an array, which a notification always carries; the Hurl line also
/// accepts a string or an object of that length
#[gtest]
#[tokio::test]
async fn e046_22_09_02_check_that_a_notification_is_sent_with_matching_entity_when_deleting_via_update_attributes_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let deleted = StreetBuildingAttribute::Name;
    let (notification, building, _subscription) = street_building_deletion_notification(&namespace, &id, deleted, NullWrite::UpdateAttributes).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(matches_document(street_building_after_deletion(&namespace, deleted)).hiding_everywhere("id"))
    );

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}

/// Pins: Delete an attribute via Update Partial Attribute Update operation and check the received notification
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The `length` check requires `$.data` to be an array, which a notification always carries; the Hurl line also
/// accepts a string or an object of that length
#[gtest]
#[tokio::test]
async fn e046_22_09_03_check_that_a_notification_is_sent_with_matching_entity_when_deleting_via_partial_attribute_update_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let deleted = StreetBuildingAttribute::Name;
    let (notification, building, _subscription) = street_building_deletion_notification(&namespace, &id, deleted, NullWrite::PartialAttributeUpdate).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(matches_document(street_building_after_deletion(&namespace, deleted)).hiding_everywhere("id"))
    );

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
