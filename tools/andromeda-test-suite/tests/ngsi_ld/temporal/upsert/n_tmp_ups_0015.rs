//! Every attribute posted gains its new instance.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: posting new instances of two attributes an existing Temporal Evolution holds answers 204 and each
/// attribute then holds its earlier instance and its new one
/// Clause: 5.6.11.4, 5.6.12.4, 6.18.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0015_every_attribute_posted_gains_its_new_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(80).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(81).observed_at("2020-08-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(70).observed_at("2020-08-01T12:05:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
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
                len(eq(2)),
                contains(any_attribute().value(120).observed_at("2020-08-01T12:03:00Z".parse()?)),
                contains(any_attribute().value(120)).times(eq(1)),
                contains(any_attribute().value(81).observed_at("2020-08-01T12:05:00Z".parse()?)),
                contains(any_attribute().value(81)).times(eq(1))
            )
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "fuelLevel",
            all!(
                len(eq(2)),
                contains(any_attribute().value(80).observed_at("2020-08-01T12:03:00Z".parse()?)),
                contains(any_attribute().value(80)).times(eq(1)),
                contains(any_attribute().value(70).observed_at("2020-08-01T12:05:00Z".parse()?)),
                contains(any_attribute().value(70)).times(eq(1))
            )
        )
    );
    Ok(())
}
