//! An existing entity gains a new attribute.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: posting an attribute an existing Temporal Evolution does not hold answers 204 and adds that attribute with
/// all its instances, the existing attribute unchanged
/// Clause: 5.6.11.4, 5.6.12.4, 6.18.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0014_an_existing_entity_gains_a_new_attribute() -> Result<()> {
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
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-08-01T13:05:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(120)))));
    expect_that!(
        evolution,
        has_evolution(
            "fuelLevel",
            all!(
                len(eq(2)),
                contains(any_attribute().value(67).observed_at("2020-08-01T12:03:00Z".parse()?)),
                contains(any_attribute().value(67)).times(eq(1)),
                contains(any_attribute().value(53).observed_at("2020-08-01T13:05:00Z".parse()?)),
                contains(any_attribute().value(53)).times(eq(1))
            )
        )
    );
    Ok(())
}
