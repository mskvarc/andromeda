//! A geoquery narrows the candidates to the entities in the area.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{geoquery, temporal_query},
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with a near geoquery maps the Entity whose
/// location observed in the interval is near and leaves out the one that is far, and without the geoquery maps both
/// Clause: 5.14.5.4, 4.10
#[gtest]
#[tokio::test]
async fn n_map_qte_7008_a_geoquery_narrows_the_candidates_to_the_entities_in_the_area() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_near_id = namespace.urn("Vehicle", "near")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_far_id = namespace.urn("Vehicle", "far")?;
    let vehicle_near_evolution = TemporalEntity::builder()
        .id(vehicle_near_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(0.0, 0.0)?)).observed_at("2021-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_near_evolution).send().await?.created()?;
    let vehicle_far_evolution = TemporalEntity::builder()
        .id(vehicle_far_id)
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(80.0, 80.0)?)).observed_at("2021-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_far_evolution).send().await?.created()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(0.0, 0.0)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(1)));
    expect_that!(map.entity_map.get(&vehicle_near_id), some(contains(eq(&EntitySource::Local))));
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
