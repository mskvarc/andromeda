//! Dataset id none beside a named instance keeps both instances.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, NgsiLdMediaType, Property},
};

/// Pins: Create Entity with one instance of an attribute carrying "`datasetId`": "@none" and another carrying a URI
/// `datasetId` keeps two instances, the default one and the named one
/// Clause: 4.5.5, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_2302_dataset_id_none_beside_a_named_instance_keeps_both_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Dataset", "gps")?;
    let default_dataset = DatasetId::DefaultInstance;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).dataset_id("@none".parse::<DatasetId>()?))
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "gps")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_instance("speed", property().value(1).without(Member::DatasetId)));
    expect_that!(entity, has_instance("speed", any_attribute().value(2).with_dataset_id(gps_dataset)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .dataset_ids(vec![default_dataset])
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(1).without(Member::DatasetId)));
    Ok(())
}
