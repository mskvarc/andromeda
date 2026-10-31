//! Each dataset instance reads back its own expires at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: two instances of one Property, each with its `datasetId` and its own `expiresAt`, read back as two
/// instances each carrying its own `expiresAt`
/// Clause: 4.22, 4.5.5, 6.3.11, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_0041_each_dataset_instance_reads_back_its_own_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speedometer_dataset = namespace.dataset_id("Dataset", "speedometer")?;
    let gps_dataset = namespace.dataset_id("Dataset", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(80)
                .dataset_id(namespace.dataset_id("Dataset", "speedometer")?)
                .expires_at("2099-12-31T23:59:59Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(78)
                .dataset_id(namespace.dataset_id("Dataset", "gps")?)
                .expires_at("2098-06-15T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute().dataset_id(speedometer_dataset).expires_at("2099-12-31T23:59:59Z".parse()?)
        )
    );
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(gps_dataset).expires_at("2098-06-15T00:00:00Z".parse()?))
    );
    Ok(())
}
