//! Deleting every instance of an attribute of 50000 instances is prompt and notified once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::{Duration, Instant};

/// The instances of the attribute.
const INSTANCES: i32 = 50_000;

/// Pins: Creating an Entity whose attribute holds 50,000 instances, then deleting every instance, each answer within
/// its bound, and the deletion is notified once with every deleted instance
/// Clause: 5.6.5.4, 5.8.6
#[gtest]
#[tokio::test]
async fn a_att_del_9234_deleting_every_instance_of_an_attribute_of_50000_instances_is_prompt_and_notified_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let mut builder = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "bus");
    for instance in 0..INSTANCES {
        builder = builder.property("speed", Property::builder(instance).dataset_id(namespace.dataset_id("Dataset", instance)?));
    }
    let vehicle = builder.build()?;
    let started = Instant::now();
    broker.entities().create(&vehicle).send().await?.done()?;
    expect_that!(started.elapsed(), lt(Duration::from_secs(10)));
    let started_2 = Instant::now();
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .instances(InstanceSelection::All)
        .send()
        .await?
        .done()?;
    expect_that!(started_2.elapsed(), lt(Duration::from_secs(5)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, has_attribute("name", any_attribute().value("bus")));
    // Clause 5.8.6: every deleted instance is reported, each by its datasetId.
    let reported = [
        BodyPattern::Selects(format!("$.data[?length(@.speed) == {INSTANCES}]").parse()?),
        BodyPattern::Selects(format!("$.data[0].speed[?(@.datasetId == 'urn:ngsi-ld:Dataset:{namespace}-{}')]", INSTANCES - 1).parse()?),
    ];
    sink.wait_for_at_least_where(1, &reported, 1).await?;
    expect_that!(sink.received_where(1, &reported).await?, len(eq(1)));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.notification().output().times_sent, some(eq(1)));
    Ok(())
}
