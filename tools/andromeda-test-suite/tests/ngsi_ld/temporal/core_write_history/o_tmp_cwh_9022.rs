//! A replace narrowing the types keeps every type in the evolution.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: a Replace Entity turning a recorded Vehicle into a Car leaves a current Entity typed Car alone, which a
/// Core query on Vehicle no longer lists, while its Temporal Evolution holds Vehicle and Car and a temporal query
/// on Vehicle still lists it
/// Clause: 5.6.18.4, 4.3.1, 5.6.11.4, 5.7.4.4
/// Recorded: clause 4.3.1 leaves to the broker how a Core API write updates the Temporal Evolution; Andromeda
/// records it, and since no operation removes a type from a Temporal Evolution (clauses 5.6.11.4 and 5.6.12.4 only
/// add), the history recorded while the Entity was a Vehicle stays reachable by Vehicle
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9022_a_replace_narrowing_the_types_keeps_every_type_in_the_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2024-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(80).observed_at("2024-07-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().replace(&vehicle_replacement).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.entity_type.as_slice(), len(eq(2)));
    expect_that!(evolution.entity_type.as_slice(), contains(eq(&vehicle_type)));
    expect_that!(evolution.entity_type.as_slice(), contains(eq(&car_type)));
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
