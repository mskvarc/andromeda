//! A dataset id selects the geometry instance.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity as `GeoJSON` with `datasetId` naming the second of two dataset instances of location
/// answers a Feature with that instance as geometry, and without it the default instance
/// Clause: 4.5.16.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9011_a_dataset_id_selects_the_geometry_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let two_dataset = namespace.dataset_id("Dataset", "two")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(9.0, 0.0)?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(1.0, 0.0)?)).dataset_id(namespace.dataset_id("Dataset", "one")?),
        )
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
        .dataset_ids(vec![two_dataset])
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(feature.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 2)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .send()
        .await?;
    let feature_2 = retrieved_2.body();
    expect_that!(feature_2.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 9)));
    Ok(())
}
