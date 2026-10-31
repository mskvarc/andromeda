//! The instance is removed and a repeat is resource not found.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute instance answers 204 with no body and removes that instance only; the same deletion
/// repeated is refused with 404 `ResourceNotFound`
/// Clause: 5.6.15.4, 6.22.3.2
#[gtest]
#[tokio::test]
async fn n_tmp_idl_0001_the_instance_is_removed_and_a_repeat_is_resource_not_found() -> Result<()> {
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
    let instance_id = instance_id_where(evolution, &"speed".parse()?, any_attribute().value(120))?;
    let received = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance_id)
        .send()
        .await?;
    expect_that!(received.body.len(), eq(0));
    let answer = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(80)))));
    expect_that!(evolution_2, has_evolution("fuelLevel", len(eq(3))));
    Ok(())
}
