//! A relationship instance reads back as a relationship.

use andromeda_test_suite::{attribute_matcher::relationship, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Relationship, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution created with a Relationship instance reads back with that instance as a Relationship
/// holding its object and `observedAt`
/// Clause: 5.6.11.4, 4.5.8, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0010_a_relationship_instance_reads_back_as_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_p1_id = namespace.urn("Parking", "P1")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .relationship(
            "isParkedIn",
            Relationship::builder(parking_p1_id.clone()).observed_at("2020-08-01T12:03:00Z".parse()?),
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
    expect_that!(
        evolution,
        has_evolution(
            "isParkedIn",
            all!(
                len(eq(1)),
                nth(0, relationship().object(parking_p1_id.clone()).observed_at("2020-08-01T12:03:00Z".parse()?))
            )
        )
    );
    Ok(())
}
