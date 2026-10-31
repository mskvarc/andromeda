//! A concise notification of a deleted geo property instance of a dataset keeps its type.

use andromeda_test_suite::{body_json::json_equals, broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{
        AttributeName,
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        Representation,
        geometry::{Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Attribute with `datasetId` gps of a `GeoProperty` whose one instance is of the dataset gps notifies a
/// concise Subscription with `attributeDeleted` of the `GeoProperty` as an object with type `GeoProperty`, value
/// `urn:ngsi-ld:null` and `datasetId` gps, since the typeless object would read back as a Property
/// Clause: 5.8.6, 4.5.2.3, 4.7, 5.6.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9241_a_concise_notification_of_a_deleted_geo_property_instance_of_a_dataset_keeps_its_type() -> Result<()> {
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
        .geo_property(
            "position",
            GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.5)?)).dataset_id(gps_dataset.clone()),
        )
        .property("speed", 40)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["position"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Concise)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"position".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&gps_dataset))
        .send()
        .await?
        .done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let body = &notifications.first().or_fail()?.body;
    // That the concise deletion writes `type` beside the NGSI-LD Null is the subject, so the notification is read as
    // received.
    expect_that!(body, json_equals("$.data[0].position.type", "GeoProperty"));
    expect_that!(body, json_equals("$.data[0].position.value", "urn:ngsi-ld:null"));
    expect_that!(
        body,
        json_equals("$.data[0].position.datasetId", format!("urn:ngsi-ld:Dataset:{namespace}-gps"))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    Ok(())
}
