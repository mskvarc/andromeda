//! Sys attrs stamp every deleted instance with deleted at.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::{every_instance, has_attribute, has_instances, member_instances},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, DatasetId, Entity, EntityFragment, Lossless, NgsiLdMediaType, Property, Relationship, Representation},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SystemAttributeReporting},
};

/// Pins: Delete Attribute with `deleteAll` of a Property with three instances notifies a Subscription with
/// notification `sysAttrs` true with all three instances nullified, each carrying `deletedAt`
/// Clause: 5.6.5.4, 5.8.6, 4.5.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9006_sys_attrs_stamp_every_deleted_instance_with_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let room_id = namespace.urn("Room", 1)?;
    let room_2_id = namespace.urn("Room", 2)?;
    let subscription_id = namespace.subscription_id(1)?;
    let french_dataset = namespace.dataset_id("Dataset", "french")?;
    let german_dataset = namespace.dataset_id("Dataset", "german")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("label", "Main")
        .property("label", Property::builder("Principal").dataset_id(namespace.dataset_id("Dataset", "french")?))
        .property("label", Property::builder("Haupt").dataset_id(namespace.dataset_id("Dataset", "german")?))
        .relationship("hosts", room_id)
        .relationship("hosts", Relationship::builder(room_2_id).dataset_id(namespace.dataset_id("Dataset", "french")?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["label"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .system_attributes(SystemAttributeReporting::Include)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker
        .entities()
        .delete_attribute(&building_id, &"label".parse::<AttributeName>()?)
        .instances(InstanceSelection::All)
        .send()
        .await?
        .done()?;
    let fragment = EntityFragment::builder().property("label", "Again").property("marker", "barrier").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_id, &"label".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    // The barrier deletion is owed the next notification and is the only one carrying marker; notifications of one subscription are delivered in order, so a second notification of the deleteAll would be in the journal before it.
    let selected = [BodyPattern::Contains("\"marker\"".to_owned())];
    sink.wait_for_at_least_where(1, &selected, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(2)));
    let selected_2 = [BodyPattern::DoesNotContain("\"marker\"".to_owned())];
    let notifications_2 = sink.received_where(1, &selected_2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_instances("label", 3)));
    expect_that!(
        notification
            .data()
            .as_slice()
            .first()
            .and_then(|item| member_instances(item, "label", Member::DatasetId)),
        some(eq(2))
    );
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(item, has_attribute("label", any_attribute().dataset_id(french_dataset)));
    expect_that!(item, has_attribute("label", any_attribute().dataset_id(german_dataset)));
    expect_that!(item, has_attribute("label", any_attribute().dataset_id(DatasetId::DefaultInstance)));
    expect_that!(item, every_instance("label", any_attribute().null_value()));
    expect_that!(item, every_instance("label", property()));
    expect_that!(
        notification
            .data()
            .as_slice()
            .first()
            .and_then(|item| member_instances(item, "label", Member::DeletedAt)),
        some(eq(3))
    );
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_id)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
