//! A geo property whose every instance is distant does not match.

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

/// Pins: a near query on `observationPoint` with a three-ordinate reference does not match an entity whose every
/// `observationPoint` instance is far away, though its location is at the reference, and matches the entity that
/// has an instance near
/// Clause: 4.10, 4.5.5, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_0041_a_geo_property_whose_every_instance_is_distant_does_not_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tower_hit_id = namespace.urn("Tower", "hit")?;
    let tower_type = namespace.entity_type("Tower")?;
    let tower_miss_id = namespace.urn("Tower", "miss")?;
    let entities = [
        Entity::builder()
            .id(tower_hit_id.clone())
            .entity_type(tower_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(30.0, 10.0)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(30.0, 10.0)?))
            .geo_property(
                "observationPoint",
                GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?)).dataset_id(namespace.dataset_id("Dataset", "sensor-a")?),
            )
            .build()?,
        Entity::builder()
            .id(tower_miss_id)
            .entity_type(tower_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(30.0, 10.0)?))
            .geo_property(
                "observationPoint",
                GeoProperty::builder(Shape::Point(Position::try_new(31.0, 11.0)?)).dataset_id(namespace.dataset_id("Dataset", "sensor-a")?),
            )
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
    expect_that!(page.items(), contains(has_id(&tower_hit_id)));
    Ok(())
}
