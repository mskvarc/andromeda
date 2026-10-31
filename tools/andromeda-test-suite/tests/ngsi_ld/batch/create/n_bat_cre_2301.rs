//! Dataset id none is the default instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Creation accepts "`datasetId`": "@none" on a Property and creates the entity with the default
/// instance
/// Clause: 4.5.5, 5.6.7
#[gtest]
#[tokio::test]
async fn n_bat_cre_2301_dataset_id_none_is_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).dataset_id("@none".parse::<DatasetId>()?))
        .build()?];
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
    expect_that!(created, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1).without(Member::DatasetId)));
    Ok(())
}
