//! A dataset id deletes only that dataset.

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
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute with a `datasetId` answers 204 with no body and removes the instances of that dataset
/// only: the default instances stay
/// Clause: 5.6.13.4, 6.21.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_adl_0002_a_dataset_id_deletes_only_that_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;

    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let fuel = namespace.dataset_id("Dataset", "fuel")?;
    let received = broker
        .with_responses()
        .temporal()
        .delete_attribute(&vehicle_id, &"fuelLevel".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&fuel))
        .send()
        .await?;
    expect_that!(received.body.len(), eq(0));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "fuelLevel",
            all!(
                len(eq(2)),
                contains(any_attribute().value(67)),
                contains(any_attribute().value(53)),
                not(contains(any_attribute().with_member(Member::DatasetId)))
            )
        )
    );
    Ok(())
}
