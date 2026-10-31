//! Near measures a multi polygon from its nearest member.

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

/// Pins: A `MultiPolygon` whose first member has its nearest wall 100.06 m from the reference point, the second
/// 2003.89 m, and whose planar centroid is about a kilometre away matches near;maxDistance==150
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_9008_near_measures_a_multi_polygon_from_its_nearest_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_building_pair_id = namespace.urn("NearBuilding", "pair")?;
    let near_building_type = namespace.entity_type("NearBuilding")?;
    let near_building_pair = Entity::builder()
        .id(near_building_pair_id.clone())
        .entity_type(near_building_type.clone())
        .geo_property(
            "location",
            Shape::MultiPolygon(vec![
                Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(8.808_693, 53.075_171)?,
                    Position::try_new(8.810_783, 53.075_171)?,
                    Position::try_new(8.810_783, 53.076_429)?,
                    Position::try_new(8.808_693, 53.076_429)?,
                    Position::try_new(8.808_693, 53.075_171)?,
                ])?])?,
                Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(8.7753, 53.0748)?,
                    Position::try_new(8.7773, 53.0748)?,
                    Position::try_new(8.7773, 53.0768)?,
                    Position::try_new(8.7753, 53.0768)?,
                    Position::try_new(8.7753, 53.0748)?,
                ])?])?,
            ]),
        )
        .build()?;
    broker.entities().create(&near_building_pair).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(150, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_pair_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&near_building_pair_id)));
    Ok(())
}
