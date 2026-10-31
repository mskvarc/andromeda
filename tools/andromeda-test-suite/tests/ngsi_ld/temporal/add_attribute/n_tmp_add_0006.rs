//! Instances join an existing attribute.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes with two instances of an Attribute the Temporal Evolution already holds answers 204, and
/// the Attribute then holds its two earlier and two new instances
/// Clause: 5.6.12.4, 6.20.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_add_0006_instances_join_an_existing_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
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
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(90).observed_at("2020-09-01T12:10:00Z".parse()?))
        .property("speed", Property::builder(95).observed_at("2020-09-01T12:15:00Z".parse()?))
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
        has_evolution(
            "speed",
            all!(
                len(eq(4)),
                contains(any_attribute().value(120).value(120)),
                contains(any_attribute().value(120)).times(eq(1)),
                contains(any_attribute().value(80).value(80)),
                contains(any_attribute().value(80)).times(eq(1)),
                contains(any_attribute().value(90).observed_at("2020-09-01T12:10:00Z".parse()?)),
                contains(any_attribute().value(90)).times(eq(1)),
                contains(any_attribute().value(95).observed_at("2020-09-01T12:15:00Z".parse()?)),
                contains(any_attribute().value(95)).times(eq(1))
            )
        )
    );
    Ok(())
}
