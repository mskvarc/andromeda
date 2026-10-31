//! The default instance is the geometry in every representation.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::projection::Projection,
};

/// Pins: Retrieve Entity as `GeoJSON` of an Entity whose location has a default instance between two dataset
/// instances answers a Feature whose geometry is the default instance, in the normalized and the simplified
/// representation, with and without a projection
/// Clause: 4.5.16.1, 4.5.17.1, 6.3.6
#[gtest]
#[tokio::test]
async fn n_ent_ret_9009_the_default_instance_is_the_geometry_in_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(1.0, 0.0)?)).dataset_id(namespace.dataset_id("Dataset", "one")?),
        )
        .geo_property("location", Shape::Point(Position::try_new(9.0, 0.0)?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(2.0, 0.0)?)).dataset_id(namespace.dataset_id("Dataset", "two")?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(feature.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 9)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_2 = retrieved_2.body();
    expect_that!(feature_2.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 9)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .omit("scope".parse::<Projection>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_3 = retrieved_3.body();
    expect_that!(feature_3.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 9)));
    Ok(())
}
