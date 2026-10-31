//! A temporal near query selects by any in window location instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        Property,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{geoquery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with georel near;maxDistance==2000 around a point 335 m from the
/// second location instance of one Vehicle and far from its first answers that Vehicle and not one without a
/// location
/// Clause: 4.10, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9013_a_temporal_near_query_selects_by_any_in_window_location_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_near_id = namespace.urn("Vehicle", "near")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_without_id = namespace.urn("Vehicle", "without")?;
    let vehicle_near_evolution = TemporalEntity::builder()
        .id(vehicle_near_id.clone())
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
    let vehicle_without_evolution = TemporalEntity::builder()
        .id(vehicle_without_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-10-01T12:04:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_without_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(-8.503, 41.202)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_near_id)));
    Ok(())
}
