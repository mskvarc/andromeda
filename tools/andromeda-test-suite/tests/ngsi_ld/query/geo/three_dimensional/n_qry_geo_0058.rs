//! A geojson feature takes the altitude of the geometry property it names.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity as geo+json with `geometryProperty` makes the three-dimensional `observationPoint` the
/// Feature geometry, altitude included, though location is two-dimensional
/// Clause: 4.5.16.1, 6.3.15, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0058_a_geojson_feature_takes_the_altitude_of_the_geometry_property_it_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(0.0, 0.0)?))
        .geo_property("observationPoint", Shape::Point(Position::try_with_altitude(13.4, 52.52, 640.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"observationPoint".parse::<AttributeName>()?)
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(
        feature.geometry.as_ref(),
        some(
            geometry()
                .length([], 3)
                .ordinate([0], Value::Number("13.4".parse()?))
                .ordinate([1], Value::Number("52.52".parse()?))
                .ordinate([2], 640)
        )
    );
    Ok(())
}
