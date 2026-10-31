//! Deleting one dataset instance records no deletion.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute of the v instance with a `datasetId` answers 204 and current state keeps the default
/// instance alone, while the Temporal Evolution holds both seeded instances and no deletion
/// Clause: 4.3.1, 5.6.5, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0012_deleting_one_dataset_instance_records_no_deletion() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let secondary_dataset = namespace.dataset_id("Dataset", "secondary")?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type)
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property(
            "v",
            Property::builder(10)
                .dataset_id(namespace.dataset_id("Dataset", "secondary")?)
                .observed_at("2026-07-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker_not_recorded.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    broker_not_recorded
        .entities()
        .delete_attribute(&sensor_id, &"v".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&secondary_dataset))
        .send()
        .await?
        .done()?;
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("v", any_attribute().value(1).without(Member::DatasetId)));
    let retrieved_2 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(2)),
                contains(
                    any_attribute()
                        .value(1)
                        .observed_at("2026-07-01T00:00:00Z".parse()?)
                        .with_member(Member::InstanceId)
                ),
                contains(
                    any_attribute()
                        .value(10)
                        .observed_at("2026-07-01T00:00:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    Ok(())
}
