//! An expired geo property instance does not match.

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

/// Pins: A location instance past its `expiresAt` no longer matches a geo-query, whether it is the default instance
/// or a dataset instance, while an identical instance that stands does
/// Clause: 4.10, 4.22
#[gtest]
#[tokio::test]
async fn n_qry_geo_0016_an_expired_geo_property_instance_does_not_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_expired_default_id = namespace.urn("Building", "expired-default")?;
    let building_type = namespace.entity_type("Building")?;
    let building_expired_tagged_id = namespace.urn("Building", "expired-tagged")?;
    let building_standing_id = namespace.urn("Building", "standing")?;
    let entities = [
        Entity::builder()
            .id(building_expired_default_id)
            .entity_type(building_type.clone())
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?)).expires_at("2000-01-01T00:00:00Z".parse()?),
            )
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(20.0, 40.0)?)).dataset_id(namespace.dataset_id("Dataset", "gps")?),
            )
            .build()?,
        Entity::builder()
            .id(building_expired_tagged_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(20.0, 40.0)?))
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?))
                    .dataset_id(namespace.dataset_id("Dataset", "gps")?)
                    .expires_at("2000-01-01T00:00:00Z".parse()?),
            )
            .build()?,
        Entity::builder()
            .id(building_standing_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(20.0, 40.0)?))
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?))
                    .dataset_id(namespace.dataset_id("Dataset", "gps")?)
                    .expires_at("2099-01-01T00:00:00Z".parse()?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
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
    expect_that!(page.items(), nth(0, has_id(&building_standing_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(13.4, 52.52)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_standing_id)));
    Ok(())
}
