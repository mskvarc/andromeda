//! A new attribute answers 204 and reads back with every instance.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes with a new Attribute of three instances answers 204 with no body, and the Temporal
/// Evolution then holds the three instances beside the existing Attributes
/// Clause: 5.6.12.4, 6.20.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_add_0001_a_new_attribute_answers_204_and_reads_back_with_every_instance() -> Result<()> {
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
        .property("added_attribute", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("added_attribute", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("added_attribute", Property::builder(100).observed_at("2020-09-01T12:07:00Z".parse()?))
        .build()?
        .attributes;
    let received = broker.with_responses().temporal().add_attributes(&vehicle_id, &instances).send().await?;
    expect_that!(received.body.len(), eq(0));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_id));
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    expect_that!(evolution, has_evolution("fuelLevel", len(eq(3))));
    expect_that!(
        evolution,
        has_evolution(
            "added_attribute",
            all!(
                len(eq(3)),
                contains(any_attribute().value(120).observed_at("2020-09-01T12:03:00Z".parse()?)),
                contains(any_attribute().value(120)).times(eq(1)),
                contains(any_attribute().value(80).observed_at("2020-09-01T12:05:00Z".parse()?)),
                contains(any_attribute().value(80)).times(eq(1)),
                contains(any_attribute().value(100).observed_at("2020-09-01T12:07:00Z".parse()?)),
                contains(any_attribute().value(100)).times(eq(1)),
                contains(property().value(100)),
                contains(any_attribute().value(100)).times(eq(1))
            )
        )
    );
    Ok(())
}
