//! A geo query on a custom geo property reads that geo property of the history.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
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

/// Pins: a temporal query whose `geoQ` names `observationPoint` answers the entity whose `observationPoint` history
/// is near the reference, 800 m up, and not the entity whose location is near and whose `observationPoint` is far
/// Clause: 4.10, 4.11, 5.7.4
/// Recorded: clause 4.10 applies to a temporal query as to a current-state one (5.7.4), and defers to OGC 06-103r4;
/// clause 4.10 defers the geometric relationships to OGC 06-103r4, whose clause 6.1.2.5 says spatial operations
/// work in the "map geometry" of the data and "will therefore not reflect z or m values in calculations" and whose
/// clause 6.1.15.1 defines the relationships by the projection of the objects onto the horizontal surface, so a
/// georel that ignores the altitude is the specified behaviour; the user ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0013_a_geo_query_on_a_custom_geo_property_reads_that_geo_property_of_the_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_near_id = namespace.urn("Building", "near")?;
    let building_type = namespace.entity_type("Building")?;
    let building_far_id = namespace.urn("Building", "far")?;
    let building_near_evolution = TemporalEntity::builder()
        .id(building_near_id.clone())
        .entity_type(building_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(30.0, 10.0)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .geo_property(
            "observationPoint",
            GeoProperty::builder(Shape::Point(Position::try_with_altitude(13.4, 52.52, 800.0)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_near_evolution).send().await?.created()?;
    let building_far_evolution = TemporalEntity::builder()
        .id(building_far_id)
        .entity_type(building_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .geo_property(
            "observationPoint",
            GeoProperty::builder(Shape::Point(Position::try_with_altitude(30.0, 10.0, 0.0)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_far_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)).geoproperty("observationPoint"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_near_id)));
    Ok(())
}
