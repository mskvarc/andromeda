//! Geoproperty names the geo property the geo query applies to.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// Pins: geoproperty names the `GeoProperty` a geo-query is applied to, alone, whether entrance, dock or
/// `observationSpace`, and without geoproperty the query applies to location
/// Clause: 4.10, 5.7.2, 6.4.3.2, 4.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0013_geoproperty_names_the_geo_property_the_geo_query_applies_to() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let quay_two_properties_id = namespace.urn("Quay", "two-properties")?;
    let quay_type = namespace.entity_type("Quay")?;
    let quay_dock_only_id = namespace.urn("Quay", "dock-only")?;
    let building_custom_id = namespace.urn("Building", "custom")?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(quay_two_properties_id.clone())
            .entity_type(quay_type.clone())
            .geo_property("entrance", Shape::Point(Position::try_new(13.0, 52.0)?))
            .geo_property(
                "entrance",
                GeoProperty::builder(Shape::Point(Position::try_new(20.0, 60.0)?)).dataset_id(namespace.dataset_id("Dataset", "remote")?),
            )
            .geo_property("dock", Shape::Point(Position::try_new(1.0, 1.0)?))
            .build()?,
        Entity::builder()
            .id(quay_dock_only_id.clone())
            .entity_type(quay_type.clone())
            .geo_property("dock", Shape::Point(Position::try_new(13.0, 52.0)?))
            .build()?,
        Entity::builder()
            .id(building_custom_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.0, 0.0)?))
            .geo_property("observationSpace", Shape::Point(Position::try_new(13.5, 52.5)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Both Quays hold a point at 13/52, under different GeoProperties.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(
            geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(12.9, 51.9)?,
                Position::try_new(13.1, 51.9)?,
                Position::try_new(13.1, 52.1)?,
                Position::try_new(12.9, 52.1)?,
                Position::try_new(12.9, 51.9)?,
            ])?])?))
            .geoproperty("entrance"),
        )
        .entity_type(&quay_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&quay_two_properties_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(
            geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(12.9, 51.9)?,
                Position::try_new(13.1, 51.9)?,
                Position::try_new(13.1, 52.1)?,
                Position::try_new(12.9, 52.1)?,
                Position::try_new(12.9, 51.9)?,
            ])?])?))
            .geoproperty("dock"),
        )
        .entity_type(&quay_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&quay_dock_only_id)));
    // An entity without entrance matches no relation over it, not even disjoint.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::disjoint(Shape::Point(Position::try_new(0.0, 0.0)?)).geoproperty("entrance"))
        .entity_type(&quay_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&quay_two_properties_id)));
    // location is at 0/0; only observationSpace is inside.
    let page_4 = broker
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
            .geoproperty("location"),
        )
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    // Without geoproperty the query applies to location.
    let page_5 = broker
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
    expect_that!(page_5.items(), len(eq(0)));
    let page_6 = broker
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
            .geoproperty("observationSpace"),
        )
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&building_custom_id)));
    Ok(())
}
