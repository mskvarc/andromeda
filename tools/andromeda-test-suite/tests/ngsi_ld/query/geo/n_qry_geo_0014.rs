//! An entity without the target geo property does not match.

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

/// Pins: An entity that does not convey the `GeoProperty` a geo-query targets does not match, and naming an absent
/// attribute or a plain Property as geoproperty is no error
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_0014_an_entity_without_the_target_geo_property_does_not_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
        .property("category", "office")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The control: location is inside.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.0, 52.0)?,
            Position::try_new(14.0, 52.0)?,
            Position::try_new(14.0, 53.0)?,
            Position::try_new(13.0, 53.0)?,
            Position::try_new(13.0, 52.0)?,
        ])?])?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(
            geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(13.0, 52.0)?,
                Position::try_new(14.0, 52.0)?,
                Position::try_new(14.0, 53.0)?,
                Position::try_new(13.0, 53.0)?,
                Position::try_new(13.0, 52.0)?,
            ])?])?))
            .geoproperty("doesNotExist"),
        )
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    // category is a Property, not a GeoProperty.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100_000, Shape::Point(Position::try_new(13.4, 52.52)?)).geoproperty("category"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100_000, Shape::Point(Position::try_new(13.4, 52.52)?)).geoproperty("absentSpace"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    Ok(())
}
