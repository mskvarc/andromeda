//! Deleted relationship instances are nullified in their object.

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
    http::operation::InstanceSelection,
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property, Relationship, Representation},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Attribute with `deleteAll` of a Relationship with a default and a French instance notifies a
/// Subscription watching it with both instances, each a Relationship whose object is the NGSI-LD Null and which
/// carries no value, the French one with its `datasetId`
/// Clause: 5.6.5.4, 5.8.6, 4.5.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9008_deleted_relationship_instances_are_nullified_in_their_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let french = namespace.dataset_id("Dataset", "french")?;
    let german = namespace.dataset_id("Dataset", "german")?;
    let hosts = "hosts".parse::<AttributeName>()?;
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
        .watched_attributes(["hosts"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker
        .entities()
        .delete_attribute(&building_id, &hosts)
        .instances(InstanceSelection::All)
        .send()
        .await?
        .done()?;
    let fragment = EntityFragment::builder()
        .relationship("hosts", namespace.urn("Room", 3)?)
        .property("marker", "barrier")
        .build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    broker.entities().delete_attribute(&building_id, &hosts).send().await?.done()?;
    // The barrier deletion is owed the next notification and is the only one carrying marker; notifications of one
    // subscription are delivered in order, so a second notification of the deleteAll would be in the journal before it.
    sink.wait_for_at_least_where(1, &[BodyPattern::Contains("\"marker\"".to_owned())], 1).await?;
    expect_that!(sink.received(1).await?, len(eq(2)));
    let deletion = sink.received_where(1, &[BodyPattern::DoesNotContain("\"marker\"".to_owned())]).await?;
    expect_that!(deletion, len(eq(1)));
    // That each deleted instance writes `object` and no `value` is the subject, and the model reads both into one
    // Relationship, so the deletion is read as received.
    let body = &deletion.first().or_fail()?.body;
    expect_that!(body, json_count("$.data", 1));
    expect_that!(body, json_equals("$.data[0].id", building_id.to_string()));
    expect_that!(body, json_count("$.data[0].hosts", 2));
    expect_that!(body, json_equals("$.data[0].hosts[*].datasetId", french.to_string()));
    expect_that!(body, json_absent("$.data[0].hosts[*].value"));
    expect_that!(body, json_absent("$.data[0].hosts[?@.object != 'urn:ngsi-ld:null']"));
    expect_that!(body, json_absent("$.data[0].hosts[?@.type != 'Relationship']"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
