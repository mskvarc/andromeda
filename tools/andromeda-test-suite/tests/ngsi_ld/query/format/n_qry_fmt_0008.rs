//! Simplified geojson flattens the feature properties.

use andromeda_test_suite::{
    broker,
    geometry_matcher::geometry,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
    type_matcher::has_type,
};
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

/// Pins: Query Entities as simplified geo+json flattens each Feature properties to bare attribute values and keeps
/// the location as the geometry
/// Clause: 4.5.17.2
#[gtest]
#[tokio::test]
async fn n_qry_fmt_0008_simplified_geojson_flattens_the_feature_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("height", 330)
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json_simplified()
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
                .ordinate([0], Value::Number("2.2945".parse()?))
                .ordinate([1], Value::Number("48.8584".parse()?))
        )
    );
    expect_that!(simplified_member_count(&feature.entity), eq(5));
    expect_that!(&feature.entity, has_type(&building_type));
    expect_that!(&feature.entity, has_value("name", "Eiffel Tower"));
    expect_that!(&feature.entity, has_value("height", 330));
    Ok(())
}
