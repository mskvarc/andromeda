//! Dataset id none in the fragment addresses the default instance.

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
    model::{Attribute, AttributeName, DatasetId, Entity, Lossless, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update whose Fragment carries "`datasetId`": "@none" updates the default instance and
/// leaves the named instance alone
/// Clause: 4.5.5, 5.6.4
#[gtest]
#[tokio::test]
async fn n_att_pat_2301_dataset_id_none_in_the_fragment_addresses_the_default_instance() -> Result<()> {
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
    let patch = Attribute::from(Property::builder(10).dataset_id("@none".parse::<DatasetId>()?).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, patch)
        .form(Lossless::Concise)
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
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_instance("speed", property().value(10).without(Member::DatasetId)));
    expect_that!(entity, has_instance("speed", any_attribute().value(2).with_dataset_id(gps_dataset)));
    Ok(())
}
