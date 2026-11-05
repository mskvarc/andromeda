//! A batch merge removing a member notifies every removed instance once.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::{every_instance, has_instance, has_instances},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Property, Representation},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A Batch Entity Merge setting speed to the NGSI-LD Null on an entity holding three instances of it sends
/// one `attributeDeleted` notification whose speed lists the three instances, the default, A and B, each with the
/// NGSI-LD Null as value
/// Clause: 5.8.6, 5.6.20.4, 5.5.12, 4.5.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9001_a_batch_merge_removing_a_member_notifies_every_removed_instance_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let notified_type = namespace.entity_type("NullNotified")?;
    let notified_id = namespace.urn("NullNotified", 1)?;
    let notified_2_id = namespace.urn("NullNotified", 2)?;
    let dataset_a = namespace.dataset_id("Dataset", "A")?;
    let dataset_b = namespace.dataset_id("Dataset", "B")?;
    let notified = Entity::builder()
        .id(notified_id.clone())
        .entity_type(notified_type.clone())
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(dataset_a.clone()))
        .property("speed", Property::builder(3).dataset_id(dataset_b.clone()))
        .property("brand", "x")
        .build()?;
    broker.entities().create(&notified).send().await?.done()?;
    let notified_2 = Entity::builder()
        .id(notified_2_id.clone())
        .entity_type(notified_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&notified_2).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([notified_type.as_str()])
        .watched_attributes(["speed"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let removal = [EntityFragment::for_entity(notified_id.clone(), notified_type.clone()).delete("speed").build()?];
    broker.entities().batch_merge(&removal).send().await?.done()?;
    // Notifications of different entities arrive in no promised order, so each is awaited by its body.
    let removal_2 = [EntityFragment::for_entity(notified_2_id.clone(), notified_type.clone())
        .delete("speed")
        .build()?];
    broker.entities().batch_merge(&removal_2).send().await?.done()?;
    let second = sink.wait_for_at_least_where(1, &[BodyPattern::Contains(notified_2_id.to_string())], 1).await?;
    expect_that!(second, len(eq(1)));
    let first = sink.wait_for_at_least_where(1, &[BodyPattern::Contains(notified_id.to_string())], 1).await?;
    expect_that!(first, len(eq(1)));
    expect_that!(sink.received(1).await?, len(eq(2)));
    let notification = first.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data(),
        elements_are![all!(
            has_instances("speed", 3),
            has_instance("speed", property().with_dataset_id(dataset_a).null_value()),
            has_instance("speed", property().with_dataset_id(dataset_b).null_value()),
            has_instance("speed", property().without(Member::DatasetId).null_value()),
            every_instance("speed", property().null_value())
        )]
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
