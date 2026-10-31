//! A subscription dataset id keeps only that deleted instance.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    body_pattern::BodyPattern,
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property, Relationship, Representation},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Attribute with `deleteAll` of a Property with a default, a French and a German instance notifies a
/// Subscription with `datasetId` `[French]` once, with the French instance alone, a single Property whose value is
/// the NGSI-LD Null
/// Clause: 5.6.5.4, 5.8.6, 4.5.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9004_a_subscription_dataset_id_keeps_only_that_deleted_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let french = namespace.dataset_id("Dataset", "french")?;
    let german = namespace.dataset_id("Dataset", "german")?;
    let label = "label".parse::<AttributeName>()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("label", "Main")
        .property("label", Property::builder("Principal").dataset_id(french.clone()))
        .property("label", Property::builder("Haupt").dataset_id(german))
        .relationship("hosts", namespace.urn("Room", 1)?)
        .relationship("hosts", Relationship::builder(namespace.urn("Room", 2)?).dataset_id(french.clone()))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["label"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .dataset_ids(vec![french.clone()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker
        .entities()
        .delete_attribute(&building_id, &label)
        .instances(InstanceSelection::All)
        .send()
        .await?
        .done()?;
    // The marker carries the dataset the Subscription selects, so the barrier notification keeps it.
    let fragment = EntityFragment::builder()
        .property("label", Property::builder("Encore").dataset_id(french.clone()))
        .property("marker", Property::builder("barrier").dataset_id(french.clone()))
        .build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_id, &label)
        .instances(InstanceSelection::Dataset(&french))
        .send()
        .await?
        .done()?;
    // The barrier deletion is owed the next notification and is the only one carrying marker; notifications of one
    // subscription are delivered in order, so a second notification of the deleteAll would be in the journal before it.
    sink.wait_for_at_least_where(1, &[BodyPattern::Contains("\"marker\"".to_owned())], 1).await?;
    expect_that!(sink.received(1).await?, len(eq(2)));
    let deletion = sink.received_where(1, &[BodyPattern::DoesNotContain("\"marker\"".to_owned())]).await?;
    expect_that!(deletion, len(eq(1)));
    // The members the deleted instance writes beside the NGSI-LD Null are counted, which the model keeps no count of,
    // so the deletion is read as received.
    let body = &deletion.first().or_fail()?.body;
    expect_that!(body, json_count("$.data", 1));
    expect_that!(body, json_equals("$.data[0].id", building_id.to_string()));
    expect_that!(body, json_count("$.data[0].label.*", 3));
    expect_that!(body, json_equals("$.data[0].label.type", "Property"));
    expect_that!(body, json_equals("$.data[0].label.value", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].label.datasetId", french.to_string()));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
