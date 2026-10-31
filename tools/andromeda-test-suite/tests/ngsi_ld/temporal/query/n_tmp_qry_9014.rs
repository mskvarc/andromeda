//! A temporal geo query selects entities without trimming instances.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{geoquery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities selecting a Vehicle by a near geoquery answers both of its in-window
/// location instances, near and far
/// Clause: 4.10, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9014_a_temporal_geo_query_selects_entities_without_trimming_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_near_id = namespace.urn("Vehicle", "near")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_near_evolution = TemporalEntity::builder()
        .id(vehicle_near_id)
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(2.35, 42.22)?)).observed_at("2020-10-01T12:03:00Z".parse()?),
        )
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)).observed_at("2020-10-01T12:05:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_near_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(-8.503, 41.202)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_evolution("location", len(eq(2)))));
    Ok(())
}
