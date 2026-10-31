//! Dataset id none is the default instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution accepts "`datasetId`": "@none" on an instance as the default instance
/// Clause: 4.5.5, 5.6.12
#[gtest]
#[tokio::test]
async fn n_tmp_add_2301_dataset_id_none_is_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Mercedes").observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(55)
                .dataset_id("@none".parse::<DatasetId>()?)
                .observed_at("2020-08-01T13:00:00Z".parse()?),
        )
        .build()?
        .attributes;
    broker.temporal().add_attributes(&vehicle_id, &instances).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(55).without(Member::DatasetId))))
    );
    Ok(())
}
