//! A point in a hole is measured to the hole boundary.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// Pins: A reference point inside the hole of a Polygon, 100.06 m from the hole boundary and at the Polygon
/// centroid, matches near;maxDistance==150 and near;minDistance==50, and not near;maxDistance==50
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_9006_a_point_in_a_hole_is_measured_to_the_hole_boundary() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_building_holed_id = namespace.urn("NearBuilding", "holed")?;
    let near_building_type = namespace.entity_type("NearBuilding")?;
    let near_building_holed = Entity::builder()
        .id(near_building_holed_id.clone())
        .entity_type(near_building_type.clone())
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![
                LinearRing::try_new(vec![
                    Position::try_new(8.7922, 53.0668)?,
                    Position::try_new(8.8222, 53.0668)?,
                    Position::try_new(8.8222, 53.0848)?,
                    Position::try_new(8.7922, 53.0848)?,
                    Position::try_new(8.7922, 53.0668)?,
                ])?,
                LinearRing::try_new(vec![
                    Position::try_new(8.805_707, 53.074_902)?,
                    Position::try_new(8.808_693, 53.074_902)?,
                    Position::try_new(8.808_693, 53.076_698)?,
                    Position::try_new(8.805_707, 53.076_698)?,
                    Position::try_new(8.805_707, 53.074_902)?,
                ])?,
            ])?),
        )
        .build()?;
    broker.entities().create(&near_building_holed).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(50, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_holed_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(150, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_holed_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&near_building_holed_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(50, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_holed_id.clone()])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&near_building_holed_id)));
    Ok(())
}
