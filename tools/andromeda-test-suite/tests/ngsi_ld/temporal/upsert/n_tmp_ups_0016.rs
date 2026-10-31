//! An existing entity gains a relationship.

use andromeda_test_suite::{attribute_matcher::relationship, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Relationship, evolution::TemporalEntity},
};

/// Pins: posting a Relationship instance for an existing Temporal Evolution answers 204 and the history then holds
/// that Relationship beside the earlier Property
/// Clause: 5.6.11.4, 5.6.12.4, 4.5.8
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0016_an_existing_entity_gains_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_p1_id = namespace.urn("Parking", "P1")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .relationship(
            "isParkedIn",
            Relationship::builder(parking_p1_id.clone()).observed_at("2020-08-01T13:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    expect_that!(
        evolution,
        has_evolution(
            "isParkedIn",
            all!(
                len(eq(1)),
                nth(0, relationship().object(parking_p1_id.clone()).observed_at("2020-08-01T13:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
