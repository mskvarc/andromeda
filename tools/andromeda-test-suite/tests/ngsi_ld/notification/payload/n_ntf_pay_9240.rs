//! A key values notification of a deleted single instance of a dataset keeps its dataset id.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, Entity, NgsiLdMediaType, Property, Representation},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Attribute with `datasetId` gps of a Property whose one instance is of the dataset gps notifies a
/// `keyValues` Subscription with `attributeDeleted` of the Property as the dataset map holding `urn:ngsi-ld:null`
/// under gps, not the bare `urn:ngsi-ld:null` that reports the whole attribute deleted
/// Clause: 5.8.6, 4.5.4, 5.6.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9240_a_key_values_notification_of_a_deleted_single_instance_of_a_dataset_keeps_its_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let gps_dataset = namespace.dataset_id("Dataset", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("owner", Property::builder("Ada").dataset_id(gps_dataset.clone()))
        .property("speed", 40)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["owner"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"owner".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&gps_dataset))
        .send()
        .await?
        .done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let body = &notifications.first().or_fail()?.body;
    // How the `keyValues` dataset map spells the deletion of one instance is the subject, so the notification is read
    // as received.
    expect_that!(
        body,
        json_equals(&format!("$.data[0].owner.dataset['urn:ngsi-ld:Dataset:{namespace}-gps']"), "urn:ngsi-ld:null")
    );
    expect_that!(body, json_absent("$.data[0].owner.dataset['@none']"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    Ok(())
}
