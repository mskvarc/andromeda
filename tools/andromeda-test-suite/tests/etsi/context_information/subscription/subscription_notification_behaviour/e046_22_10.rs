//! Deleting the `locatedAt` Relationship by writing the NGSI-LD Null with Merge Entity, Update Attributes or Partial
//! Attribute Update notifies a normalized subscription watching it with the `attributeDeleted` trigger, the attribute
//! reported as a Relationship whose object is the NGSI-LD Null (clause 5.8.6).

use crate::context_information::subscription::subscription_notification_behaviour::{
    deletion_case::{NullWrite, StreetBuildingAttribute, street_building_after_deletion, street_building_deletion_notification},
    notification_case::teardown,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::subscription::SubscriptionId;
use serde_json::{Value, json};

/// Pins: Delete a relationship via Merge Entity operation and check the received notification
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_22_10_01_check_that_a_notification_is_sent_with_matching_entity_when_deleting_via_merge_entity_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let deleted = StreetBuildingAttribute::LocatedAt;
    let (notification, building, _subscription) = street_building_deletion_notification(&namespace, &id, deleted, NullWrite::MergeEntity).await?;
    let body = notification.json()?;
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

/// Pins: Delete a relationship via Update Attributes operation and check the received notification
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_22_10_02_check_that_a_notification_is_sent_with_matching_entity_when_deleting_via_update_attributes_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let deleted = StreetBuildingAttribute::LocatedAt;
    let (notification, building, _subscription) = street_building_deletion_notification(&namespace, &id, deleted, NullWrite::UpdateAttributes).await?;
    let body = notification.json()?;
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

/// Pins: Delete a relationship via Update Partial Attribute Update operation and check the received notification
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_22_10_03_check_that_a_notification_is_sent_with_matching_entity_when_deleting_via_partial_attribute_update_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let deleted = StreetBuildingAttribute::LocatedAt;
    let (notification, building, _subscription) = street_building_deletion_notification(&namespace, &id, deleted, NullWrite::PartialAttributeUpdate).await?;
    let body = notification.json()?;
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
