//! A temporal geo query reads only the locations inside the window as q does.

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
    query::{geoquery, q, temporal_query},
};

/// Pins: For one after window opening at 12:30, a temporal georel and a temporal q read the same instances, those
/// inside the window: of three Vehicles each with speed observed inside it, only the one whose location near the
/// reference and `brandName` Volvo are observed at 12:35 meets near and q, while neither the one whose near location
/// and Volvo are observed at 12:00 nor the one that moved far away at 12:10 does
/// Clause: 4.10, 4.11, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9076_a_temporal_geo_query_reads_only_the_locations_inside_the_window_as_q_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_parked_id = namespace.urn("Vehicle", "parked")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_moved_id = namespace.urn("Vehicle", "moved")?;
    let vehicle_parked_evolution = TemporalEntity::builder()
        .id(vehicle_parked_id)
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)).observed_at("2020-10-01T12:00:00Z".parse()?),
        )
        .property("brandName", Property::builder("Volvo").observed_at("2020-10-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(0).observed_at("2020-10-01T12:40:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_parked_evolution).send().await?.created()?;
    let vehicle_moved_evolution = TemporalEntity::builder()
        .id(vehicle_moved_id)
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)).observed_at("2020-10-01T12:00:00Z".parse()?),
        )
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(2.35, 42.22)?)).observed_at("2020-10-01T12:10:00Z".parse()?),
        )
        .property("brandName", Property::builder("Seat").observed_at("2020-10-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(0).observed_at("2020-10-01T12:40:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_moved_evolution).send().await?.created()?;
    let vehicle_arrived_id = namespace.urn("Vehicle", "arrived")?;
    let vehicle_arrived_evolution = TemporalEntity::builder()
        .id(vehicle_arrived_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)).observed_at("2020-10-01T12:35:00Z".parse()?),
        )
        .property("brandName", Property::builder("Volvo").observed_at("2020-10-01T12:35:00Z".parse()?))
        .property("speed", Property::builder(0).observed_at("2020-10-01T12:40:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_arrived_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-10-01T12:30:00Z".parse()?))
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(-8.503, 41.202)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_arrived_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-10-01T12:30:00Z".parse()?))
        .entity_type(&vehicle_type)
        .q(q::attr("brandName").eq("Volvo"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_arrived_id)));
    Ok(())
}
