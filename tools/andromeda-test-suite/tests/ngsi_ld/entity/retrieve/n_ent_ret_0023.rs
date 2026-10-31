//! Geojson takes the geometry from the named geometry property.

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

/// Pins: Retrieve Entity as geo+json with `geometryProperty` takes the Feature geometry from the named
/// `GeoProperty` instead of location
/// Clause: 4.5.16.1, 4.5.16.2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_0023_geojson_takes_the_geometry_from_the_named_geometry_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("observationPoint", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .geo_property("location", Shape::Point(Position::try_new(0.0, 0.0)?))
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
    expect_that!(feature.entity.id, eq(&building_id));
    expect_that!(
        feature.geometry.as_ref(),
        some(
            geometry()
                .of_type("Point")
                .length([], 2)
                .ordinate([0], Value::Number("13.3986".parse()?))
                .ordinate([1], Value::Number("52.5547".parse()?))
        )
    );
    Ok(())
}
