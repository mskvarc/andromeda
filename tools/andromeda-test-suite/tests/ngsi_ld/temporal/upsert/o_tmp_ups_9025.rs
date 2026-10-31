//! A recreate after a core delete keeps the evolution types and gives current state the written ones.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: after a Delete Entity of a Vehicle whose Temporal Evolution exists, a Create or Update Temporal Evolution
/// naming Car answers 204; the recreated current Entity is a Car alone, while the Temporal Evolution holds Vehicle
/// and Car and a temporal query on Vehicle still lists it
/// Clause: 5.6.11.4, 5.6.6, 4.3.1, 5.7.4.4
/// Recorded: the Temporal Evolution outlives a Core API delete, so clause 5.6.11.4 adds Car to its list and keeps
/// Vehicle; the specification gives the Temporal API no say over current state, and Andromeda gives the recreated
/// Entity the types its writer sent
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9025_a_recreate_after_a_core_delete_keeps_the_evolution_types_and_gives_current_state_the_written_ones() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2024-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(80).observed_at("2024-07-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(car_type.clone())));
    expect_that!(entity, has_attribute("speed", any_attribute().value(80)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution.entity_type.as_slice(), len(eq(2)));
    expect_that!(evolution.entity_type.as_slice(), contains(eq(&vehicle_type)));
    expect_that!(evolution.entity_type.as_slice(), contains(eq(&car_type)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
