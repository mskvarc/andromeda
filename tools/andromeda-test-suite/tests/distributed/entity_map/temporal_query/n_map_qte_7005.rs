//! A last n query still maps the entities of the interval.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with `lastN` 2 over an Entity holding three
/// observations in the interval answers 201 and maps the Entity
/// Clause: 5.14.5.4, 5.14.5.3
#[gtest]
#[tokio::test]
async fn n_map_qte_7005_a_last_n_query_still_maps_the_entities_of_the_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2021-06-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2021-12-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?).last_n(2))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(1)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
