//! A deleted list property is notified as a value list of one null.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals},
    body_pattern::BodyPattern,
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{AttributeName, Entity, ListProperty, ListRelationship, NgsiLdMediaType, Value},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::slice;

/// Pins: A `showChanges` notification of Delete Attribute of a `ListProperty` carries `valueList`
/// `["urn:ngsi-ld:null"]` and neither value nor `previousValue`
/// Clause: 4.5.0, 4.5.21.2, 5.8.6, 5.6.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9016_a_deleted_list_property_is_notified_as_a_value_list_of_one_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("deleted-list")?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "deleted-list-notified")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .list_property("tags", ListProperty::builder(vec![Value::from("residential"), Value::from("commercial")]))
        .list_relationship(
            "hasRooms",
            ListRelationship::builder(vec![namespace.urn("Room", 1)?, namespace.urn("Room", 2)?]),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_id_pattern(namespace.id_pattern("Building")?)])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("deleted-list")?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker
        .entities()
        .delete_attribute(&building_id, &"tags".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let listed = BodyPattern::Contains("valueList".to_owned());
    sink.wait_for_at_least_where("deleted-list", slice::from_ref(&listed), 1).await?;
    let notifications = sink.received_where("deleted-list", slice::from_ref(&listed)).await?;
    expect_that!(notifications, len(eq(1)));
    let body = &notifications.first().or_fail()?.body;
    // How the deleted `ListProperty` spells the NGSI-LD Null, and which members it leaves out, is the subject, so the
    // notification is read as received.
    expect_that!(body, json_count("$.data[0].tags.valueList", 1));
    expect_that!(body, json_equals("$.data[0].tags.valueList[0]", "urn:ngsi-ld:null"));
    expect_that!(body, json_absent("$.data[0].tags.value"));
    expect_that!(body, json_absent("$.data[0].tags.previousValue"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
