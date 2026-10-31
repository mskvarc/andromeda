//! A geojson feature collection keeps the arity of each position.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Query Entities as geo+json keeps the arity of each position: the Feature of a three-dimensional location
/// keeps its altitude and the one of a two-dimensional location gains none
/// Clause: 4.5.16.3, 6.3.15, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_geo_0057_a_geojson_feature_collection_keeps_the_arity_of_each_position() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_high_id = namespace.urn("Building", "high")?;
    let building_type = namespace.entity_type("Building")?;
    let building_flat_id = namespace.urn("Building", "flat")?;
    let entities = [
        Entity::builder()
            .id(building_high_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
            .build()?,
        Entity::builder()
            .id(building_flat_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    let feature = page.items().iter().find(|item| item.entity.id == building_high_id).or_fail()?;
    expect_that!(
        feature.geometry.as_ref(),
        some(
            geometry()
                .length([], 3)
                .ordinate([0], Value::Number("13.4".parse()?))
                .ordinate([1], Value::Number("52.52".parse()?))
                .ordinate([2], 300)
        )
    );
    let feature_2 = page.items().iter().find(|item| item.entity.id == building_flat_id).or_fail()?;
    expect_that!(
        feature_2.geometry.as_ref(),
        some(
            geometry()
                .length([], 2)
                .ordinate([0], Value::Number("13.4".parse()?))
                .ordinate([1], Value::Number("52.52".parse()?))
        )
    );
    Ok(())
}
