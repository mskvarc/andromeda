//! Flat as geojson is a feature collection of both entities.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::Join,
};

/// Pins: Query Entities with join=flat answered as `GeoJSON` is a `FeatureCollection` holding a Feature for the
/// matching entity and one for its Linked Entity
/// Clause: 4.5.23.3, 5.7.2.4, 5.7.2.5, 5.2.30, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0013_flat_as_geojson_is_a_feature_collection_of_both_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let entities = [
        Entity::builder()
            .id(building_b_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.0, 0.0)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(7.1, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.0, 0.0)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .relationship("linkedTo", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    page.items().iter().find(|item| item.entity.id == building_a_id).or_fail()?;
    page.items().iter().find(|item| item.entity.id == building_b_id).or_fail()?;
    Ok(())
}
