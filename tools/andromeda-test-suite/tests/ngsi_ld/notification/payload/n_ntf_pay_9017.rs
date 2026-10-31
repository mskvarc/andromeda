//! A normalized notification carries relationship objects in both lists.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals},
    body_pattern::BodyPattern,
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use serde_json::json;
use std::slice;

/// Pins: A normalized `showChanges` notification of a Partial Attribute Update replacing a `ListRelationship`
/// carries `objectList` and `previousObjectList` as Relationship Objects holding only object, and neither object
/// nor `previousObject`
/// Clause: 4.5.22.2, Table 5.2.37-1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9017_a_normalized_notification_carries_relationship_objects_in_both_lists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("normalized-list")?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "normalized-notification")?;
    let room_1 = namespace.urn_text("Room", 1);
    let room_2 = namespace.urn_text("Room", 2);
    let room_3 = namespace.urn_text("Room", 3);
    // The plain-string objectList items are the spelling under test; the model writes them as objects, so they go out as written.
    let building = json!({
        "id": building_id.to_string(),
        "type": building_type.as_str(),
        "hasRooms": {"type": "ListRelationship", "objectList": [room_1, room_2]}
    });
    let request = json_request_to(&broker, Method::POST, "entities", &building.to_string())?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::CREATED));
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_id_pattern(namespace.id_pattern("Building")?)])
        .notification_triggers([NotificationTrigger::AttributeUpdated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("normalized-list")?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let patch = json!({"type": "ListRelationship", "objectList": [room_3]});
    let request_2 = json_request_to(&broker, Method::PATCH, &format!("entities/{building_id}/attrs/hasRooms"), &patch.to_string())?;
    expect_that!(&broker.send_unchecked(request_2).await, answered(StatusCode::NO_CONTENT));
    let previous = BodyPattern::Contains("previousObjectList".to_owned());
    sink.wait_for_at_least_where("normalized-list", slice::from_ref(&previous), 1).await?;
    let notifications = sink.received_where("normalized-list", slice::from_ref(&previous)).await?;
    expect_that!(notifications, len(eq(1)));
    // How each list item is spelled is the subject, which the model reads into one shape, so the notification is read
    // as received.
    let body = &notifications.first().or_fail()?.body;
    expect_that!(body, json_count("$.data[0].hasRooms.objectList", 1));
    expect_that!(body, json_equals("$.data[0].hasRooms.objectList[0].object", room_3.as_str()));
    expect_that!(body, json_count("$.data[0].hasRooms.objectList[0].*", 1));
    expect_that!(body, json_count("$.data[0].hasRooms.previousObjectList", 2));
    expect_that!(body, json_equals("$.data[0].hasRooms.previousObjectList[0].object", room_1.as_str()));
    expect_that!(body, json_count("$.data[0].hasRooms.previousObjectList[0].*", 1));
    expect_that!(body, json_equals("$.data[0].hasRooms.previousObjectList[1].object", room_2.as_str()));
    expect_that!(body, json_count("$.data[0].hasRooms.previousObjectList[1].*", 1));
    expect_that!(body, json_absent("$.data[0].hasRooms.object"));
    expect_that!(body, json_absent("$.data[0].hasRooms.previousObject"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
