//! Geojson answers a feature collection.

use andromeda_test_suite::{
    attribute_matcher::{geo_property, property},
    broker,
    entity_matcher::{has_attribute, member_count},
    geometry_matcher::geometry,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
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

/// Pins: Query Entities as geo+json answers a `FeatureCollection` holding one Feature per selected Building, each
/// with the location as geometry and the normalized attributes as properties
/// Clause: 4.5.16.3, 6.3.4, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_fmt_0002_geojson_answers_a_feature_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let ids = [namespace.urn("Building", 1)?, namespace.urn("Building", 2)?];
    let mut entities = Vec::new();
    for id in &ids {
        entities.push(
            Entity::builder()
                .id(id.clone())
                .entity_type(building_type.clone())
                .property("name", "Eiffel Tower")
                .property("subCategory", "tourism")
                .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .ids(ids.to_vec())
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    for id in &ids {
        let feature = page.items().iter().find(|feature| &feature.entity.id == id).or_fail()?;
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
        expect_that!(&feature.entity, all!(has_id(id), has_type(&building_type)));
        expect_that!(member_count(&feature.entity), eq(5));
        expect_that!(&feature.entity, has_attribute("name", property().value("Eiffel Tower")));
        expect_that!(&feature.entity, has_attribute("subCategory", property().value("tourism")));
        expect_that!(
            &feature.entity,
            has_attribute("location", geo_property().geometry(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        );
    }
    Ok(())
}
