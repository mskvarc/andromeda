//! An upsert adds the types the evolution lacks and keeps the ones it holds.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: a Create or Update Temporal Evolution naming Car for an existing Vehicle evolution, its one instance
/// observed before the recorded one, answers 204, and the Temporal Evolution then holds both types and is listed by
/// a temporal query on Vehicle and on Car
/// Clause: 5.6.11.4, 5.7.4.4, 4.5.6
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9023_an_upsert_adds_the_types_the_evolution_lacks_and_keeps_the_ones_it_holds() -> Result<()> {
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
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(80).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
