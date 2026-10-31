//! Flat as geojson is a feature collection and geometry property applies.

use andromeda_test_suite::{broker, flat_geo_json::retrieve_flat_collection, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::model::{
    AttributeName,
    Entity,
    geometry::{Geometry, Position, Shape},
};

/// Pins: Retrieve Entity with join=flat answered as `GeoJSON` is a `FeatureCollection` holding a Feature for the
/// retrieved entity and one for its Linked Entity, each geometry taken from location, or from the `GeoProperty`
/// `geometryProperty` names
/// Clause: 5.7.1.5, 4.5.23.3, 5.2.30, 6.3.15, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0012_flat_as_geojson_is_a_feature_collection_and_geometry_property_applies() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_b_id = namespace.urn("Building", "b")?;
    let entities = [
        Entity::builder()
            .id(building_b_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(7.1, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.25)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .relationship("linkedTo", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let collection = retrieve_flat_collection(&broker, &building_a_id, 1, None).await?;
    expect_that!(collection.features, len(eq(2)));
    let feature_a = collection.features.iter().find(|feature| feature.entity.id == building_a_id).or_fail()?;
    expect_that!(
        feature_a.geometry.as_ref().map(Geometry::shape),
        some(eq(&Shape::Point(Position::try_new(0.5, 0.25)?)))
    );
    let feature_b = collection.features.iter().find(|feature| feature.entity.id == building_b_id).or_fail()?;
    expect_that!(
        feature_b.geometry.as_ref().map(Geometry::shape),
        some(eq(&Shape::Point(Position::try_new(1.5, 2.5)?)))
    );
    let observation_point = "observationPoint".parse::<AttributeName>()?;
    let collection_2 = retrieve_flat_collection(&broker, &building_a_id, 1, Some(&observation_point)).await?;
    expect_that!(collection_2.features, len(eq(2)));
    let feature_a_2 = collection_2.features.iter().find(|feature| feature.entity.id == building_a_id).or_fail()?;
    expect_that!(
        feature_a_2.geometry.as_ref().map(Geometry::shape),
        some(eq(&Shape::Point(Position::try_new(13.3986, 52.5547)?)))
    );
    let feature_b_2 = collection_2.features.iter().find(|feature| feature.entity.id == building_b_id).or_fail()?;
    expect_that!(
        feature_b_2.geometry.as_ref().map(Geometry::shape),
        some(eq(&Shape::Point(Position::try_new(7.1, 52.5547)?)))
    );
    Ok(())
}
