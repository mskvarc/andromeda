//! A values filter narrows the candidates to the matching entities.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with q speed>50 maps the Entity whose
/// observations in the interval match and leaves out the one whose do not, and without q maps both
/// Clause: 5.14.5.4, 4.9
#[gtest]
#[tokio::test]
async fn n_map_qte_7007_a_values_filter_narrows_the_candidates_to_the_matching_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_fast_id = namespace.urn("Vehicle", "fast")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_slow_id = namespace.urn("Vehicle", "slow")?;
    let vehicle_fast_evolution = TemporalEntity::builder()
        .id(vehicle_fast_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(90).observed_at("2021-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(91).observed_at("2021-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_fast_evolution).send().await?.created()?;
    let vehicle_slow_evolution = TemporalEntity::builder()
        .id(vehicle_slow_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2021-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(11).observed_at("2021-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_slow_evolution).send().await?.created()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(1)));
    expect_that!(map.entity_map.get(&vehicle_fast_id), some(contains(eq(&EntitySource::Local))));
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
    expect_that!(&map_2.entity_map, len(eq(2)));
    Ok(())
}
