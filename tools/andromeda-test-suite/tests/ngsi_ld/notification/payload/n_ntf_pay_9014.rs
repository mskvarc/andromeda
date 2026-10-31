//! Show changes on a list property carries the previous value list.

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
    model::{Attribute, AttributeName, Entity, ListProperty, ListRelationship, NgsiLdMediaType, Value},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::slice;

/// Pins: A `showChanges` notification of a Partial Attribute Update replacing a `ListProperty` `valueList` carries
/// `previousValueList` holding the replaced array, and neither value nor `previousValue`
/// Clause: 4.5.21.2, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9014_show_changes_on_a_list_property_carries_the_previous_value_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("value-list")?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "previous-value-list")?;
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
        .notification_triggers([NotificationTrigger::AttributeUpdated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("value-list")?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let patch = Attribute::from(ListProperty::builder(vec![Value::from("industrial")]).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&building_id, &"tags".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let previous = BodyPattern::Contains("previousValueList".to_owned());
    sink.wait_for_at_least_where("value-list", slice::from_ref(&previous), 1).await?;
    let notifications = sink.received_where("value-list", slice::from_ref(&previous)).await?;
    expect_that!(notifications, len(eq(1)));
    let body = &notifications.first().or_fail()?.body;
    // The members the notified `ListProperty` writes beside `previousValueList` are the subject, so the notification is
    // read as received.
    expect_that!(body, json_count("$.data[0].tags.previousValueList", 2));
    expect_that!(body, json_equals("$.data[0].tags.previousValueList[0]", "residential"));
    expect_that!(body, json_equals("$.data[0].tags.previousValueList[1]", "commercial"));
    expect_that!(body, json_absent("$.data[0].tags.previousValue"));
    expect_that!(body, json_absent("$.data[0].tags.value"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
