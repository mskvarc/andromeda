//! A new entity answers 201 with location and reads back.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{DateTime, NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution posted for an id the broker does not know is created, answers 201 with a Location naming
/// it, and reads back with every instance it was written with
/// Clause: 5.6.11.4, 6.18.3.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0001_a_new_entity_answers_201_with_location_and_reads_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let twelve_03 = "2020-08-01T12:03:00Z".parse::<DateTime>()?;
    let twelve_05 = "2020-08-01T12:05:00Z".parse::<DateTime>()?;
    let thirteen_05 = "2020-08-01T13:05:00Z".parse::<DateTime>()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at(twelve_03))
        .property("speed", Property::builder(80).observed_at(twelve_05))
        .property("fuelLevel", Property::builder(67).observed_at(twelve_03))
        .property("fuelLevel", Property::builder(53).observed_at(thirteen_05))
        .build()?;
    let created = broker.with_heads().temporal().create_or_update(&vehicle_evolution).send().await?;
    created.outcome.created()?;
    expect_that!(
        created.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(ends_with(format!("/temporal/entities/{vehicle_id}")))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(&evolution.id, eq(&vehicle_id));
    expect_that!(&evolution.entity_type, eq(&OneOrMany::one(vehicle_type)));
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(any_attribute().value(120).observed_at(twelve_03)),
                contains(any_attribute().value(80).observed_at(twelve_05))
            )
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "fuelLevel",
            all!(
                len(eq(2)),
                contains(any_attribute().value(67).observed_at(twelve_03)),
                contains(any_attribute().value(53).observed_at(thirteen_05))
            )
        )
    );
    Ok(())
}
