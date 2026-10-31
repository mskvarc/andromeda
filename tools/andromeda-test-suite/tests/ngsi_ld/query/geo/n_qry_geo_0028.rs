//! A geo query on a named geoproperty matches either dataset instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
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

/// Pins: a geo-query with geoproperty naming a `GeoProperty` held by two `datasetId` instances matches the entity
/// when either instance meets the relation, the first or the second, and not when neither does
/// Clause: 4.10, 4.5.5, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_0028_a_geo_query_on_a_named_geoproperty_matches_either_dataset_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let park_first_id = namespace.urn("Park", "first")?;
    let park_type = namespace.entity_type("Park")?;
    let park_second_id = namespace.urn("Park", "second")?;
    let park_neither_id = namespace.urn("Park", "neither")?;
    let entities = [
        Entity::builder()
            .id(park_first_id.clone())
            .entity_type(park_type.clone())
            .geo_property(
                "area",
                GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(9.0, 9.0)?,
                    Position::try_new(11.0, 9.0)?,
                    Position::try_new(11.0, 11.0)?,
                    Position::try_new(9.0, 11.0)?,
                    Position::try_new(9.0, 9.0)?,
                ])?])?))
                .dataset_id(namespace.dataset_id("Dataset", "first")?),
            )
            .geo_property(
                "area",
                GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(49.0, 49.0)?,
                    Position::try_new(51.0, 49.0)?,
                    Position::try_new(51.0, 51.0)?,
                    Position::try_new(49.0, 51.0)?,
                    Position::try_new(49.0, 49.0)?,
                ])?])?))
                .dataset_id(namespace.dataset_id("Dataset", "second")?),
            )
            .build()?,
        Entity::builder()
            .id(park_second_id.clone())
            .entity_type(park_type.clone())
            .geo_property(
                "area",
                GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(49.0, 49.0)?,
                    Position::try_new(51.0, 49.0)?,
                    Position::try_new(51.0, 51.0)?,
                    Position::try_new(49.0, 51.0)?,
                    Position::try_new(49.0, 49.0)?,
                ])?])?))
                .dataset_id(namespace.dataset_id("Dataset", "first")?),
            )
            .geo_property(
                "area",
                GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(9.0, 9.0)?,
                    Position::try_new(11.0, 9.0)?,
                    Position::try_new(11.0, 11.0)?,
                    Position::try_new(9.0, 11.0)?,
                    Position::try_new(9.0, 9.0)?,
                ])?])?))
                .dataset_id(namespace.dataset_id("Dataset", "second")?),
            )
            .build()?,
        Entity::builder()
            .id(park_neither_id)
            .entity_type(park_type.clone())
            .geo_property(
                "area",
                GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(49.0, 49.0)?,
                    Position::try_new(51.0, 49.0)?,
                    Position::try_new(51.0, 51.0)?,
                    Position::try_new(49.0, 51.0)?,
                    Position::try_new(49.0, 49.0)?,
                ])?])?))
                .dataset_id(namespace.dataset_id("Dataset", "first")?),
            )
            .geo_property(
                "area",
                GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(-51.0, -51.0)?,
                    Position::try_new(-49.0, -51.0)?,
                    Position::try_new(-49.0, -49.0)?,
                    Position::try_new(-51.0, -49.0)?,
                    Position::try_new(-51.0, -51.0)?,
                ])?])?))
                .dataset_id(namespace.dataset_id("Dataset", "second")?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(10.0, 10.0)?)).geoproperty("area"))
        .entity_type(&park_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&park_first_id)));
    expect_that!(page.items(), contains(has_id(&park_second_id)));
    Ok(())
}
