//! Dataset id none merges into the default instance.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    entity_matcher::{has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity with "`datasetId`": "@none" on a Property merges into the default instance and leaves the
/// named instance alone
/// Clause: 4.5.5, 5.6.17
#[gtest]
#[tokio::test]
async fn n_ent_mrg_2301_dataset_id_none_merges_into_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Dataset", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "gps")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(10).dataset_id("@none".parse::<DatasetId>()?))
        .build()?;
    broker.entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_instance("speed", property().value(10).without(Member::DatasetId)));
    expect_that!(entity, has_instance("speed", any_attribute().value(2).with_dataset_id(gps_dataset)));
    Ok(())
}
