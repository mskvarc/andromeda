//! An existing entity gains the new instances.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: posting further instances of an attribute for an existing Temporal Evolution answers 204 and the history
/// then holds the earlier instance and the new ones
/// Clause: 5.6.11.4, 5.6.12.4, 6.18.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0013_an_existing_entity_gains_the_new_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).observed_at("2020-08-01T12:05:00Z".parse()?))
        .property("speed", Property::builder(100).observed_at("2020-08-01T12:07:00Z".parse()?))
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
                len(eq(3)),
                contains(any_attribute().value(120).observed_at("2020-08-01T12:03:00Z".parse()?)),
                contains(any_attribute().value(120)).times(eq(1)),
                contains(any_attribute().value(80).observed_at("2020-08-01T12:05:00Z".parse()?)),
                contains(any_attribute().value(80)).times(eq(1)),
                contains(any_attribute().value(100).observed_at("2020-08-01T12:07:00Z".parse()?)),
                contains(any_attribute().value(100)).times(eq(1))
            )
        )
    );
    Ok(())
}
