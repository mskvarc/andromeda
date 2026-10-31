//! An unknown attribute is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, ErrorType, Property, evolution::TemporalEntity};

/// Pins: Delete Attribute addressed to an Attribute the Temporal Evolution does not hold is refused with 404
/// `ResourceNotFound`, with a title
/// Clause: 5.6.13.4, 6.21.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_adl_0008_an_unknown_attribute_is_resource_not_found() -> Result<()> {
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
    let answer = broker
        .with_responses()
        .temporal()
        .delete_attribute(&vehicle_id, &"nonExistentAttr".parse::<AttributeName>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
