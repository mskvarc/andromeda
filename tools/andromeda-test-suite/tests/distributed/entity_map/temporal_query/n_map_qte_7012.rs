//! With split entities an entity failing q is still a candidate.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{EntitySplitting, q, temporal_query},
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with `splitEntities` true and q speed>10 maps
/// both the local Entity whose speed is 20 and the local Entity whose speed is 5, the values filter being left to
/// the merged result
/// Clause: 5.14.5.4, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_map_qte_7012_with_split_entities_an_entity_failing_q_is_still_a_candidate() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_2_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_2_evolution).send().await?.created()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(10))
        .split_entities(EntitySplitting::Split)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map.entity_map.get(&vehicle_2_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
