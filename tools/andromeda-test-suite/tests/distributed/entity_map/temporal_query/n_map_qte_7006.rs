//! An entity without an observation in the interval is left out.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with an interval before 2020 leaves out an
/// Entity observed only in 2021, an interval before 2022 and an interval after 2020 map it
/// Clause: 5.14.5.4, 4.11
#[gtest]
#[tokio::test]
async fn n_map_qte_7006_an_entity_without_an_observation_in_the_interval_is_left_out() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(0)));
    let created_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_2 = created_2.body();
    expect_that!(&map_2.entity_map, len(eq(1)));
    expect_that!(map_2.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    let created_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_3 = created_3.body();
    expect_that!(&map_3.entity_map, len(eq(1)));
    expect_that!(map_3.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
