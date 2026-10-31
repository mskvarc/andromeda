//! An instance with a dataset id joins the default instance.

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

/// Pins: posting an instance with a datasetId for an attribute an existing Temporal Evolution holds only as a default
/// instance answers 204 and the history then holds both, the datasetId on the new instance only
/// Clause: 5.6.11.4, 5.6.12.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0019_an_instance_with_a_dataset_id_joins_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed_dataset = namespace.dataset_id("Dataset", "speed")?;
    let twelve_03 = "2020-08-01T12:03:00Z".parse::<DateTime>()?;
    let twelve_05 = "2020-08-01T12:05:00Z".parse::<DateTime>()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(100).observed_at(twelve_03))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_dataset_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(110).observed_at(twelve_05).dataset_id(speed_dataset.clone()))
        .build()?;
    broker.temporal().create_or_update(&vehicle_dataset_evolution).send().await?.updated()?;
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
                contains(any_attribute().value(110).with_dataset_id(speed_dataset).observed_at(twelve_05)),
                contains(any_attribute().value(100).without(Member::DatasetId).observed_at(twelve_03))
            )
        )
    );
    Ok(())
}
