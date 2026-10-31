//! A dataset id parameter selects the named instances.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity with a `datasetId` URL parameter answers only the Attribute instances of that `datasetId`
/// Clause: 4.5.5, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_2608_a_dataset_id_parameter_selects_the_named_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property("speed", Property::builder(60).dataset_id(namespace.dataset_id("Property", "speedometer")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .dataset_ids(vec![gps_dataset.clone()])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55).with_dataset_id(gps_dataset.clone())));
    Ok(())
}
