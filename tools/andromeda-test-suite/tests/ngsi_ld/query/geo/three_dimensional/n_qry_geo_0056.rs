//! A three dimensional point reads back as geojson with its altitude.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    geometry_matcher::geometry,
    namespace::Namespace,
    outcome::Done,
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

/// Pins: Retrieve Entity as geo+json keeps the altitude of a three-dimensional location, in the Feature geometry
/// and in the location kept under properties
/// Clause: 4.5.16.2, 6.3.15, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0056_a_three_dimensional_point_reads_back_as_geojson_with_its_altitude() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
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
                .ordinate([2], 300)
        )
    );
    let entity = &feature.entity;
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .length_at(["coordinates"], 3)
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
                .value_member(["coordinates", "2"], 300)
        )
    );
    Ok(())
}
