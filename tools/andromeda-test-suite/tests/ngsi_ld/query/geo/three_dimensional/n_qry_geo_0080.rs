//! A dataset instance of a three dimensional geo property is enough to match.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: a near query on `observationPoint` matches an entity whose default instance is far away and whose
/// `datasetId` instance, 800 m up, is within 100 m of a three-ordinate reference, and not an entity with no
/// instance near
/// Clause: 4.10, 4.5.5, 5.7.2.4
/// Recorded: clause 5.7.2.4 makes one `datasetId` instance meeting the geospatial restrictions enough; the match
/// here rests on the 800 m altitude of that instance being ignored by near, clause 4.10 defers the geometric
/// relationships to OGC 06-103r4, whose clause 6.1.2.5 says spatial operations work in the "map geometry" of the
/// data and "will therefore not reflect z or m values in calculations" and whose clause 6.1.15.1 defines the
/// relationships by the projection of the objects onto the horizontal surface, so a georel that ignores the
/// altitude is the specified behaviour; the user ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_qry_geo_0080_a_dataset_instance_of_a_three_dimensional_geo_property_is_enough_to_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tower_multi_id = namespace.urn("Tower", "multi")?;
    let tower_type = namespace.entity_type("Tower")?;
    let tower_off_id = namespace.urn("Tower", "off")?;
    let entities = [
        Entity::builder()
            .id(tower_multi_id.clone())
            .entity_type(tower_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(30.0, 10.0)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(30.0, 10.0)?))
            .geo_property(
                "observationPoint",
                GeoProperty::builder(Shape::Point(Position::try_with_altitude(13.4, 52.52, 800.0)?)).dataset_id(namespace.dataset_id("Dataset", "sensor-a")?),
            )
            .build()?,
        Entity::builder()
            .id(tower_off_id)
            .entity_type(tower_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .geo_property("observationPoint", Shape::Point(Position::try_with_altitude(30.0, 10.0, 800.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)).geoproperty("observationPoint"))
        .entity_type(&tower_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tower_multi_id)));
    Ok(())
}
