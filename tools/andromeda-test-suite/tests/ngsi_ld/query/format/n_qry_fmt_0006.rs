//! Geojson without geometry property takes location over another geo property.

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

/// Pins: Query Entities as geo+json without `geometryProperty` takes each Feature geometry from the location
/// attribute, not from another `GeoProperty` of the entity
/// Clause: 4.5.16.1, 4.5.16.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_fmt_0006_geojson_without_geometry_property_takes_location_over_another_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(2.3522, 48.8566)?))
        .geo_property("otherGeo", Shape::Point(Position::try_new(99.0, 9.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .ids(vec![building_id.clone()])
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let feature = page.items().first().or_fail()?;
    expect_that!(feature.entity.id, eq(&building_id));
    expect_that!(
        feature.geometry.as_ref(),
        some(
            geometry()
                .of_type("Point")
                .length([], 2)
                .ordinate([0], Value::Number("2.3522".parse()?))
                .ordinate([1], Value::Number("48.8566".parse()?))
        )
    );
    Ok(())
}
