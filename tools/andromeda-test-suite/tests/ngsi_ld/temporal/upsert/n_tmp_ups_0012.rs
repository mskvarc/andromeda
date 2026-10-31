//! An instance with a dataset id reads back beside the default instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution created with a default instance and an instance with a datasetId of one attribute reads
/// back with both, the datasetId kept on its instance only
/// Clause: 5.6.11.4, 4.5.5, 4.5.7, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0012_an_instance_with_a_dataset_id_reads_back_beside_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed_dataset = namespace.dataset_id("Dataset", "speed")?;
    let twelve_03 = "2020-08-01T12:03:00Z".parse::<DateTime>()?;
    let twelve_04 = "2020-08-01T12:04:00Z".parse::<DateTime>()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(100).observed_at(twelve_03))
        .property("speed", Property::builder(110).observed_at(twelve_04).dataset_id(speed_dataset.clone()))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(any_attribute().value(110).with_dataset_id(speed_dataset).observed_at(twelve_04)),
                contains(any_attribute().value(100).without(Member::DatasetId).observed_at(twelve_03))
            )
        )
    );
    Ok(())
}
