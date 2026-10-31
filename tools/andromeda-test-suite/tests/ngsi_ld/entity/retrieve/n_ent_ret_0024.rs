//! Geojson without geometry property takes the geometry from location.

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

/// Pins: Retrieve Entity as geo+json without `geometryProperty` takes the Feature geometry from the location
/// attribute
/// Clause: 4.5.16.1, 4.5.16.2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_0024_geojson_without_geometry_property_takes_the_geometry_from_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(4.9041, 52.3676)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
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
                .ordinate([0], Value::Number("4.9041".parse()?))
                .ordinate([1], Value::Number("52.3676".parse()?))
        )
    );
    Ok(())
}
