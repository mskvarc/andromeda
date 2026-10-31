//! A drop deletes every instance of the attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Purge Entities with drop naming an attribute that has a default instance and a `datasetId` instance
/// answers 204 and deletes the whole attribute, every instance of it, leaving the other attributes
/// Clause: 5.6.21.4, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_0009_a_drop_deletes_every_instance_of_the_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .property("speed", Property::builder(20).dataset_id(namespace.dataset_id("Dataset", "one")?))
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    broker
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .drop(vec!["speed".parse()?])
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
