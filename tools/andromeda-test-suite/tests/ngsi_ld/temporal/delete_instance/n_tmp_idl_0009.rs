//! An unknown instance is resource not found.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, ErrorType, InstanceId, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute instance addressed to an `instanceId` the Attribute does not hold is refused with 404
/// `ResourceNotFound`, with a title, and both instances stay
/// Clause: 5.6.15.4, 6.22.3.2
#[gtest]
#[tokio::test]
async fn n_tmp_idl_0009_an_unknown_instance_is_resource_not_found() -> Result<()> {
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
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    instance_id_where(evolution, &"speed".parse()?, any_attribute().value(120))?;
    let instance_id_2 = format!("urn:ngsi-ld:Instance:{namespace}-unknown").parse::<InstanceId>()?;
    let answer = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance_id_2)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", len(eq(2))));
    Ok(())
}
