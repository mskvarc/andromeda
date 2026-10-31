//! Deleting one instance among 50000 is prompt and notifies that instance as deleted.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
    query::projection::Projection,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::{Duration, Instant};

/// The instances of the attribute.
const INSTANCES: i32 = 50_000;

/// The instance deleted.
const DELETED: i32 = 25_000;

/// Pins: Deleting one instance of an attribute holding 50,000 answers within its bound, keeps the others, and
/// notifies that instance as deleted beside the ones held
/// Clause: 5.6.5.4, 5.8.6
#[gtest]
#[tokio::test]
async fn a_att_del_9235_deleting_one_instance_among_50000_is_prompt_and_notifies_that_instance_as_deleted() -> Result<()> {
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
    let deleted_dataset = namespace.dataset_id("Dataset", DELETED)?;
    let started_2 = Instant::now();
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&deleted_dataset))
        .send()
        .await?
        .done()?;
    expect_that!(started_2.elapsed(), lt(Duration::from_secs(5)));
    let before = namespace.dataset_id("Dataset", DELETED - 1)?;
    let after = namespace.dataset_id("Dataset", DELETED + 1)?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&vehicle_id)
        .pick("speed".parse::<Projection>()?)
        .dataset_ids(vec![before.clone(), deleted_dataset, after.clone()])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(before).value(DELETED - 1)));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(after).value(DELETED + 1)));
    // Clause 5.8.6: the notification reports the one deleted instance.
    let reported = [
        BodyPattern::Selects(
            format!("$.data[0].speed[?(@.datasetId == 'urn:ngsi-ld:Dataset:{namespace}-{DELETED}' && @.value == 'urn:ngsi-ld:null')]").parse()?,
        ),
        BodyPattern::Selects(
            format!(
                "$.data[0].speed[?(@.datasetId == 'urn:ngsi-ld:Dataset:{namespace}-{}' && @.value == {})]",
                DELETED + 1,
                DELETED + 1
            )
            .parse()?,
        ),
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
