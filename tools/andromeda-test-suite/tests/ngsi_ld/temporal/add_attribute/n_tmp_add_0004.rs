//! A relationship instance reads back as a relationship.

use andromeda_test_suite::{attribute_matcher::relationship, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Relationship, evolution::TemporalEntity},
};

/// Pins: Add Attributes with a Relationship instance answers 204, and the Temporal Evolution then holds it as a
/// Relationship with its object
/// Clause: 5.6.12.4, 6.20.3.1, 4.5.8
#[gtest]
#[tokio::test]
async fn n_tmp_add_0004_a_relationship_instance_reads_back_as_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_p1_id = namespace.urn("Parking", "P1")?;
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
        .relationship(
            "isParkedIn",
            Relationship::builder(parking_p1_id.clone()).observed_at("2020-09-01T14:00:00Z".parse()?),
        )
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
        has_evolution("isParkedIn", all!(len(eq(1)), nth(0, relationship().object(parking_p1_id.clone()))))
    );
    Ok(())
}
