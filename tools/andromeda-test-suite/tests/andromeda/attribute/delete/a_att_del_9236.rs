//! Deleting the attribute written before one of 50000 instances is prompt and keeps every instance after it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
    query::projection::Projection,
};
use std::time::{Duration, Instant};

/// The instances of the attribute after the deleted one.
const INSTANCES: i32 = 50_000;

/// Pins: Deleting the attribute an Entity holds before an attribute of 50,000 instances answers within its bound, and
/// every instance after it keeps its value
/// Clause: 5.6.5.4
#[gtest]
#[tokio::test]
async fn a_att_del_9236_deleting_the_attribute_before_50000_instances_is_prompt_and_keeps_every_instance_after_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let mut builder = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("first", "lead");
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
        .delete_attribute(&vehicle_id, &"first".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    expect_that!(started_2.elapsed(), lt(Duration::from_secs(5)));
    let first = namespace.dataset_id("Dataset", 0)?;
    let last = namespace.dataset_id("Dataset", INSTANCES - 1)?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&vehicle_id)
        .pick("speed".parse::<Projection>()?)
        .dataset_ids(vec![first.clone(), last.clone()])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(first).value(0)));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(last).value(INSTANCES - 1)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&vehicle_id)
        .pick("id,first".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(retrieved_2.body(), lacks_attribute("first"));
    Ok(())
}
