//! A named geometry property uses its own default instance.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity as `GeoJSON` with `geometryProperty` `observationSpace` answers a Feature whose geometry
/// is the default instance of `observationSpace`, not of location
/// Clause: 4.5.16.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9014_a_named_geometry_property_uses_its_own_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(9.0, 0.0)?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(1.0, 0.0)?)).dataset_id(namespace.dataset_id("Dataset", "one")?),
        )
        .geo_property(
            "observationSpace",
            GeoProperty::builder(Shape::Point(Position::try_new(5.0, 0.0)?)).dataset_id(namespace.dataset_id("Dataset", "one")?),
        )
        .geo_property("observationSpace", Shape::Point(Position::try_new(7.0, 0.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"observationSpace".parse::<AttributeName>()?)
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(feature.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 7)));
    Ok(())
}
