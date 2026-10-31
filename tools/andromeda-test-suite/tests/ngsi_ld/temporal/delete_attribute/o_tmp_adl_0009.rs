//! An unknown dataset id is resource not found.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute with a `datasetId` that no instance of the Attribute has is refused with 404
/// `ResourceNotFound` and every instance stays
/// Clause: 5.6.13.4, 6.21.3.1
/// Recorded: clause 5.6.13.4 raises `ResourceNotFound` when the Temporal Evolution does not contain the target
/// Attribute and does not say what a `datasetId` that no instance of an existing Attribute has leads to; Andromeda
/// treats the missing instances as a missing target and answers `ResourceNotFound`
#[gtest]
#[tokio::test]
async fn o_tmp_adl_0009_an_unknown_dataset_id_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let nonexistent_dataset = namespace.dataset_id("Dataset", "nonexistent")?;
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
    let answer = broker
        .with_responses()
        .temporal()
        .delete_attribute(&vehicle_id, &"fuelLevel".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&nonexistent_dataset))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("fuelLevel", len(eq(3))));
    Ok(())
}
