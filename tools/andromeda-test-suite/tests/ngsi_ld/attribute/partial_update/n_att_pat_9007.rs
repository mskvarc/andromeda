//! The fragment dataset id addresses one instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, Lossless, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update and Replace Attribute whose fragment carries a `datasetId` answer 204 and write
/// only the instance of that `datasetId`
/// Clause: 5.6.4.4, 5.6.19.4, 5.5.8
#[gtest]
#[tokio::test]
async fn n_att_pat_9007_the_fragment_dataset_id_addresses_one_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_dataset_fragment_id = namespace.urn("Vehicle", "dataset-fragment")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let speedometer_dataset = namespace.dataset_id("Property", "speedometer")?;
    let vehicle_dataset_fragment = Entity::builder()
        .id(vehicle_dataset_fragment_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property("speed", Property::builder(61).dataset_id(namespace.dataset_id("Property", "speedometer")?))
        .build()?;
    broker.entities().create(&vehicle_dataset_fragment).send().await?.done()?;
    let patch = Attribute::from(Property::builder(65).dataset_id(namespace.dataset_id("Property", "speedometer")?).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_dataset_fragment_id, &"speed".parse::<AttributeName>()?, patch)
        .form(Lossless::Concise)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_dataset_fragment_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset.clone()).value(60)));
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset.clone()).value(65))
    );
    let attribute = Attribute::from(Property::builder(100).dataset_id(namespace.dataset_id("Property", "gps")?).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_dataset_fragment_id, &"speed".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_dataset_fragment_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_instances("speed", 2));
    expect_that!(entity_2, has_attribute("speed", any_attribute().dataset_id(gps_dataset.clone()).value(100)));
    expect_that!(
        entity_2,
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset.clone()).value(65))
    );
    Ok(())
}
